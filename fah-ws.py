import curses
import time
import requests
import urllib3
import ipaddress
import re
import threading
import os
import asyncio
import json
import ssl
from collections import deque

try:
    import websockets
except ImportError:
    print("Biblioteca 'websockets' nu este instalată! Rulează: pip install websockets")
    exit(1)

urllib3.disable_warnings(urllib3.exceptions.InsecureRequestWarning)

TOKEN = "c48ac59cdaef2a10cf96ddeac66dd4d3c1be57bf1dcfbd9e96b0aed7de06ef65"
HOST = "172.17.0.2"
PORT = 8443
BASE = f"https://{HOST}:{PORT}"
H = {"Authorization": f"Bearer {TOKEN}"}

# Configurare RouterOS REST API
ROS_BASE = "https://192.168.10.1:8443/rest"
ROS_USER = "monitor"
ROS_PASS = os.getenv("MP", "")
ROS_AUTH = (ROS_USER, ROS_PASS)

ROUTER_FETCH_INTERVAL = 15

# ==========================================
# STĂRI ȘI THREAD-URI ASYNC / WS
# ==========================================
lock = threading.Lock()

ws_connected = False
ws_status_msg = "CONNECTING"
ws_queries = deque(maxlen=200)

ws_stats = {
    "queries_total": 0,
    "blocked_total": 0,
    "blocked_percent": 0.0,
    "cache_hit_percent": 0.0,
    "top_blocked_domains": [],
    "top_queried_domains": [],
    "top_clients": []
}

router_cache = {
    "last_fetch": 0,
    "free_mem": "N/A",
    "cpu_freq": "N/A",
    "cpu_load": "N/A",
    "container_mem": "N/A",
    "is_loading": False
}

perf_cache = {
    "last_fetch": 0,
    "points": []
}

PERF_FETCH_INTERVAL = 86400  # 24 ore

async def ws_listener():
    global ws_connected, ws_status_msg
    ws_url = f"wss://{HOST}:{PORT}/api/v1/events?token={TOKEN}"

    ssl_ctx = ssl.create_default_context()
    ssl_ctx.check_hostname = False
    ssl_ctx.verify_mode = ssl.CERT_NONE

    while True:
        try:
            async with websockets.connect(ws_url, ssl=ssl_ctx) as ws:
                with lock:
                    ws_connected = True
                    ws_status_msg = "ONLINE (WS)"

                while True:
                    raw = await ws.recv()
                    msg = json.loads(raw)
                    m_type = msg.get("type")
                    payload = msg.get("data", {})

                    with lock:
                        if m_type == "query":
                            ws_queries.appendleft(payload)
                        elif m_type == "stats":
                            ws_stats["queries_total"] = payload.get("queries_total", 0)
                            ws_stats["blocked_total"] = payload.get("blocked_total", 0)
                            ws_stats["blocked_percent"] = payload.get("blocked_percent", 0.0)
                            ws_stats["cache_hit_percent"] = payload.get("cache_hit_percent", 0.0)
                            ws_stats["top_blocked_domains"] = payload.get("top_blocked_domains", [])[:10]
                            ws_stats["top_queried_domains"] = payload.get("top_queried_domains", [])[:10]
                            ws_stats["top_clients"] = payload.get("top_clients", [])[:10]

        except Exception as e:
            with lock:
                ws_connected = False
                ws_status_msg = f"OFFLINE ({type(e).__name__})"
            await asyncio.sleep(2)

def start_ws_thread():
    def run():
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        loop.run_until_complete(ws_listener())
    t = threading.Thread(target=run, daemon=True)
    t.start()

# ==========================================
# FUNCȚII HELPER & METRICE
# ==========================================
def fetch_mikrotik_stats_thread():
    global router_cache
    router_cache["is_loading"] = True
    try:
        r_res = requests.get(f"{ROS_BASE}/system/resource", auth=ROS_AUTH, verify=False, timeout=2)
        if r_res.status_code == 200:
            res_data = r_res.json()
            raw_free = res_data.get("free-memory", 0)
            free_bytes = int(raw_free) if str(raw_free).isdigit() else 0
            if free_bytes > 0:
                router_cache["free_mem"] = f"{free_bytes / 1024 / 1024:.1f}MiB"
            else:
                router_cache["free_mem"] = "N/A"

            freq = res_data.get("cpu-frequency", "N/A")
            router_cache["cpu_freq"] = f"{freq}MHz" if freq != "N/A" else "N/A"
            load = res_data.get("cpu-load", "N/A")
            router_cache["cpu_load"] = f"{load}%" if load != "N/A" else "N/A"

        r_cnt = requests.get(f"{ROS_BASE}/container", auth=ROS_AUTH, verify=False, timeout=2)
        if r_cnt.status_code == 200:
            containers = r_cnt.json()
            if isinstance(containers, list):
                for item in containers:
                    if item.get("name") == "fastadhunter":
                        raw_mem = item.get("memory-current", 0)
                        cnt_bytes = int(raw_mem) if str(raw_mem).isdigit() else 0
                        if cnt_bytes > 0:
                            router_cache["container_mem"] = f"{cnt_bytes / 1024 / 1024:.1f}MiB"
                        else:
                            router_cache["container_mem"] = str(raw_mem) if raw_mem else "N/A"
                        break

        router_cache["last_fetch"] = time.time()
    except Exception:
        pass
    finally:
        router_cache["is_loading"] = False

def update_router_stats_if_needed():
    now = time.time()
    if now - router_cache["last_fetch"] >= ROUTER_FETCH_INTERVAL and not router_cache["is_loading"]:
        t = threading.Thread(target=fetch_mikrotik_stats_thread, daemon=True)
        t.start()

def fmtip(s):
    try:
        ip = ipaddress.ip_address(s)
        return str(ip.ipv4_mapped) if getattr(ip, "ipv4_mapped", None) else str(s)
    except:
        return str(s)

def bar(p, w=10):
    if p <= 0:
        return "░" * w
    blocks = ["░", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"]
    total_units = p / 100 * w
    full_blocks = int(total_units)
    remainder = int((total_units - full_blocks) * 8)
    
    if full_blocks == 0 and remainder == 0:
        remainder = 1

    res = "█" * full_blocks
    if full_blocks < w:
        res += blocks[remainder]
        res += "░" * (w - full_blocks - 1)
    return res[:w]

def fetch_perf_history():
    global perf_cache
    now = time.time()
    if now - perf_cache["last_fetch"] < PERF_FETCH_INTERVAL and perf_cache["points"]:
        return perf_cache["points"]

    try:
        endpoints = ["/api/v1/history/perf?fields=rss_bytes", "/history/perf?fields=rss_bytes"]
        for ep in endpoints:
            r = requests.get(BASE + ep, headers=H, verify=False, timeout=3)
            if r.status_code == 200:
                data = r.json()
                raw_pts = []

                for item in data.get("items", []):
                    val_bytes = item.get("rss_bytes", 0)
                    if val_bytes > 0:
                        raw_pts.append(val_bytes / 1024 / 1024)

                if raw_pts:
                    perf_cache["points"] = raw_pts
                    perf_cache["last_fetch"] = now
                    break
    except:
        pass

    return perf_cache["points"]

def draw_multi_row_braille(data, width, height=3):
    if not data or width <= 0:
        return [" " * width for _ in range(height)]

    min_v, max_v = min(data), max(data)
    total_pts = len(data)
    
    total_sub_cols = width * 2
    sampled = []
    for i in range(total_sub_cols):
        idx = int(i * total_pts / total_sub_cols)
        sampled.append(data[min(idx, total_pts - 1)])

    total_dots_y = height * 4
    grid = [[False] * total_sub_cols for _ in range(total_dots_y)]

    for sub_x, val in enumerate(sampled):
        if max_v == min_v:
            h_dots = 1
        else:
            raw_h = ((val - min_v) / (max_v - min_v)) * (total_dots_y - 1) + 1
            h_dots = max(1, min(total_dots_y, int(raw_h)))
        
        for y in range(h_dots):
            grid[total_dots_y - 1 - y][sub_x] = True

    rows_text = []
    dot_map = [
        [0x1, 0x8],
        [0x2, 0x10],
        [0x4, 0x20],
        [0x40, 0x80]
    ]

    for r in range(height):
        row_str = ""
        y_offset = r * 4
        for c in range(width):
            sub_x0 = c * 2
            sub_x1 = sub_x0 + 1
            
            char_code = 0x2800
            for dy in range(4):
                gy = y_offset + dy
                if gy < total_dots_y:
                    if grid[gy][sub_x0]:
                        char_code |= dot_map[dy][0]
                    if grid[gy][sub_x1]:
                        char_code |= dot_map[dy][1]
            
            row_str += chr(char_code)
        rows_text.append(row_str)

    return rows_text

def j(path):
    try:
        r = requests.get(BASE + path, headers=H, verify=False, timeout=1)
        if r.status_code == 200:
            return r.json(), True
        return {}, False
    except:
        return {}, False

def parse_metrics():
    try:
        r = requests.get(BASE + "/metrics", headers=H, verify=False, timeout=1)
        if r.status_code != 200:
            return {}, 0, "0.000 ms", "0.000 ms"
        
        rules = 0
        upstreams = {}
        sum_block, cnt_block = 0.0, 0
        sum_cache, cnt_cache = 0.0, 0

        for line in r.text.splitlines():
            if line.startswith("fastadhunter_ruleset_rules"):
                parts = line.split()
                if len(parts) >= 2:
                    rules = int(parts[1])
            elif line.startswith("fastadhunter_upstream_attempts_total"):
                m = re.search(r'address="([^"]+)"', line)
                parts = line.split()
                if m and len(parts) >= 2:
                    upstreams[m.group(1)] = int(parts[1])
            elif line.startswith('fastadhunter_query_duration_seconds_sum{stage="block"}'):
                sum_block = float(line.split()[1])
            elif line.startswith('fastadhunter_query_duration_seconds_count{stage="block"}'):
                cnt_block = int(line.split()[1])
            elif line.startswith('fastadhunter_query_duration_seconds_sum{stage="cache_hit"}'):
                sum_cache = float(line.split()[1])
            elif line.startswith('fastadhunter_query_duration_seconds_count{stage="cache_hit"}'):
                cnt_cache = int(line.split()[1])

        avg_block = f"{(sum_block / cnt_block * 1000):.3f} ms" if cnt_block > 0 else "0.000 ms"
        avg_cache = f"{(sum_cache / cnt_cache * 1000):.3f} ms" if cnt_cache > 0 else "0.000 ms"

        return upstreams, rules, avg_block, avg_cache
    except:
        return {}, 0, "0.000 ms", "0.000 ms"

def is_allowed(item):
    v = str(item.get("verdict", item.get("action", ""))).lower()
    return v in ["allow", "pass", "permitted"]

# ==========================================
# MAIN LOOP
# ==========================================
def main(stdscr):
    curses.curs_set(0)
    stdscr.nodelay(True)
    
    curses.start_color()
    curses.use_default_colors()

    curses.init_pair(1, curses.COLOR_GREEN, -1)
    curses.init_pair(2, curses.COLOR_RED, -1)
    curses.init_pair(5, curses.COLOR_CYAN, -1)

    use_256 = curses.COLORS >= 256
    if use_256:
        curses.init_pair(3, curses.COLOR_WHITE, 22)
        curses.init_pair(4, curses.COLOR_WHITE, 52)

    start_ws_thread()

    while True:
        ch = stdscr.getch()
        if ch == curses.KEY_RESIZE:
            stdscr.clear()
        if ch == ord('q') or ch == ord('Q'):
            stdscr.clear()
            stdscr.refresh()
            break

        curses.update_lines_cols()

        update_router_stats_if_needed()

        stdscr.erase()

        max_y, max_x = stdscr.getmaxyx()
        w = max(80, max_x - 2)

        c_data, ok2 = j("/api/v1/cache")
        m_data, ok3 = j("/api/v1/debug/memory")
        upstreams, rules_count, avg_block_str, avg_cache_str = parse_metrics()
        
        perf_pts = fetch_perf_history()
        
        with lock:
            is_online = ws_connected and ok2 and ok3
            st_msg = ws_status_msg
            q = list(ws_queries)
            st = dict(ws_stats)

        c = c_data
        m = m_data

        h = c.get("hits", 0)
        ms = c.get("misses", 0)
        hp = (h * 100 / (h + ms)) if (h + ms) else 0

        rss = m.get("process_rss", 0) / 1024 / 1024
        peak = m.get("process_peak_rss", 0) / 1024 / 1024
        ruleset_mb = m.get("ruleset_bytes", 0) / 1024 / 1024
        alloc_peak_mb = m.get("allocator_committed_peak_bytes", 0) / 1024 / 1024

        c_bytes = c.get("bytes", 0) / 1024 / 1024
        c_max_bytes = c.get("max_bytes", 0) / 1024 / 1024
        load = c.get("load_percent", 0)
        fresh = c.get("fresh", 0)
        stale = c.get("stale", 0)

        display_pts = list(perf_pts) + [rss] if perf_pts else [rss]

        ups_str = " │ ".join([f"{ip}: {cnt}" for ip, cnt in upstreams.items()]) if upstreams else "N/A"

        c1_rss   = f"  RSS   {bar(min(rss, 150) / 150 * 100, 18)} {rss:5.1f} MB"
        c2_rss   = f"Peak {peak:5.1f} MB"
        
        part1_rss   = f"Ruleset {ruleset_mb:4.1f} MB"
        c3_rss      = f"{part1_rss:<15} │ Alloc Peak {alloc_peak_mb:5.1f} MB"

        c1_hit   = f"  Hit   {bar(hp, 18)} {hp:5.1f}%"
        c2_hit   = f"Hit: {h} - Miss: {ms}"
        
        part1_hit   = f"DNS L: {avg_block_str}"
        c3_hit      = f"{part1_hit:<15} │ Cache Hit: {avg_cache_str}"

        c1_cache = f"  Cache {bar(load, 18)} {load:5.1f}%"
        c2_cache = f"{c.get('entries', 0)}/{c.get('capacity', 0)} ({c_bytes:.1f}/{c_max_bytes:.0f} MB)"
        
        part1_cache = f"Fresh {fresh}"
        c3_cache    = f"{part1_cache:<15} │ Stale {stale}"

        left_part_l3 = f"│{c1_rss:<36}│ {c2_rss:<28}│ {c3_rss:<38}"
        left_part_l4 = f"│{c1_hit:<36}│ {c2_hit:<28}│ {c3_hit:<38}"
        left_part_l5 = f"│{c1_cache:<36}│ {c2_cache:<28}│ {c3_cache:<38}"

        graph_width = w - len(left_part_l3) - 1
        if graph_width < 5:
            graph_width = 0

        # Header
        t_str = time.strftime('%H:%M:%S')
        status_txt = f"● {st_msg}"
        status_color = curses.color_pair(1) if is_online else curses.color_pair(2)
        
        stdscr.addstr(0, 0, "┌" + "─" * w + "┐")
        stdscr.addstr(1, 0, "│ ")
        stdscr.addstr("FastAdHunter Monitor", curses.color_pair(5))
        stdscr.addstr("      ")
        stdscr.addstr(status_txt, status_color)
        
        rules_txt = f" │ Rules: {rules_count:,}" if rules_count else ""
        stdscr.addstr(rules_txt)
        
        space_hdr = w - 28 - len(status_txt) - len(rules_txt) - len(t_str)
        stdscr.addstr(" " * max(0, space_hdr) + f"{t_str} │")

        min_rss = min(display_pts)
        avg_rss = sum(display_pts) / len(display_pts)
        now_rss = rss

        lbl_graph = f" 24h RSS History (Min {min_rss:.0f} │ Avg {avg_rss:.0f} │ Now {now_rss:.0f} MB) "
        if graph_width > len(lbl_graph):
            p_left = (graph_width - len(lbl_graph)) // 2
            p_right = graph_width - len(lbl_graph) - p_left
            graph_hdr = "─" * p_left + lbl_graph + "─" * p_right
        else:
            graph_hdr = "─" * graph_width

        left_sep = "├" + "─" * (len(left_part_l3) - 1) + "┬"
        stdscr.addstr(2, 0, left_sep + graph_hdr)
        stdscr.addch(2, w + 1, '┤')

        graph_rows = draw_multi_row_braille(display_pts, graph_width, height=3)

        stdscr.addstr(3, 0, left_part_l3)
        stdscr.addstr("│", curses.A_DIM)
        if graph_width > 0:
            stdscr.addstr(graph_rows[0], curses.color_pair(1))
        stdscr.addch(3, w + 1, '│')

        stdscr.addstr(4, 0, left_part_l4)
        stdscr.addstr("│", curses.A_DIM)
        if graph_width > 0:
            stdscr.addstr(graph_rows[1], curses.color_pair(1))
        stdscr.addch(4, w + 1, '│')

        stdscr.addstr(5, 0, left_part_l5)
        stdscr.addstr("│", curses.A_DIM)
        if graph_width > 0:
            stdscr.addstr(graph_rows[2], curses.color_pair(1))
        stdscr.addch(5, w + 1, '│')

        # ------------------------------------------------------------------
        # SPLIT SCREEN SETUP
        # ------------------------------------------------------------------
        left_w = 48
        right_w = w - left_w - 1

        stdscr.addstr(6, 0, "├" + "─" * left_w + "┬" + "─" * right_w + "┤")
        stdscr.addstr(7, 0, f"│{' Live Metrics & Top Stats (24h) ':.^{left_w}}│{' Live Queries Feed ':.^{right_w}}│")
        stdscr.addstr(8, 0, "├" + "─" * left_w + "┼" + "─" * right_w + "┤")

        client_col_w = 42
        domain_width = max(10, right_w - (client_col_w + 34))
        q_hdr_str = f"  {'Client':<{client_col_w}} {'Domain':<{domain_width}} {'Type':<8} {'Verdict':<10} {'Time':<8}"
        
        stdscr.addstr(9, 0, "│")
        stdscr.addstr(9, 1, " " * left_w)
        stdscr.addstr(9, left_w + 1, "│")
        stdscr.addstr(9, left_w + 2, f"{q_hdr_str:<{right_w-1}}", curses.color_pair(5) | curses.A_UNDERLINE | curses.A_BOLD)
        stdscr.addch(9, w + 1, '│')

        max_rows = max(5, max_y - 15)

        stats_lines = []
        stats_lines.append((f" Blocked Rate: {bar(st['blocked_percent'], 10)} {st['blocked_percent']:5.1f}%", curses.color_pair(2) | curses.A_BOLD))
        stats_lines.append((f" Cache Hit:    {bar(st['cache_hit_percent'], 10)} {st['cache_hit_percent']:5.1f}%", curses.color_pair(1)))
        stats_lines.append((f" Total: {st['queries_total']:,} │ Blocked: {st['blocked_total']:,}", curses.A_DIM))
        stats_lines.append(("", 0))

        if st["top_blocked_domains"]:
            stats_lines.append(("── Top Blocked Domains (Top 10) ──", curses.color_pair(5) | curses.A_BOLD))
            for b_item in st["top_blocked_domains"][:10]:
                d_name = str(b_item.get("domain", ""))[:left_w - 12]
                stats_lines.append((f" • {d_name:<{left_w-12}} {b_item.get('count', 0):>6}", 0))
            stats_lines.append(("", 0))

        if st["top_clients"]:
            stats_lines.append(("── Top Clients (Top 10) ──", curses.color_pair(5) | curses.A_BOLD))
            for c_item in st["top_clients"][:10]:
                ip_str = fmtip(c_item.get("ip", c_item.get("client", "")))[:left_w - 12]
                stats_lines.append((f" • {ip_str:<{left_w-12}} {c_item.get('count', 0):>6}", 0))

        for idx in range(max_rows):
            r_y = 10 + idx
            if r_y >= max_y - 4: break

            stdscr.addstr(r_y, 0, "│")

            if idx < len(stats_lines):
                txt, attr = stats_lines[idx]
                stdscr.addstr(r_y, 1, f"{txt:<{left_w}}", attr)
            else:
                stdscr.addstr(r_y, 1, " " * left_w)

            stdscr.addstr(r_y, left_w + 1, "│")

            q_idx = idx
            if q_idx < len(q):
                it = q[q_idx]
                client = fmtip(str(it.get('client', '')))[ :client_col_w]
                domain = str(it.get('domain', ''))[:domain_width]
                qtype = str(it.get('qtype', ''))[:8]
                
                allowed = is_allowed(it)
                v_char = "Pass" if allowed else "Block"
                
                row_attr = curses.color_pair(3) if (use_256 and allowed) else (curses.color_pair(4) if use_256 else (curses.color_pair(1) if allowed else curses.color_pair(2)))
                
                raw_ts = str(it.get("ts", ""))
                ts_display = raw_ts.split("T")[-1].split(".")[0] if "T" in raw_ts else raw_ts[:8]
                if not ts_display:
                    dur_val = it.get('duration_ms', 0)
                    ts_display = f"{dur_val:8.3f}" if isinstance(dur_val, (int, float)) else str(dur_val)[:8]

                row_content = f"  {client:<{client_col_w}} {domain:<{domain_width}} {qtype:<8} {v_char:<10} {ts_display:<8}"
                stdscr.addstr(r_y, left_w + 2, f"{row_content:<{right_w-1}}", row_attr)
            else:
                stdscr.addstr(r_y, left_w + 2, " " * (right_w - 1))

            stdscr.addch(r_y, w + 1, '│')

        footer_r = 10 + max_rows
        if footer_r < max_y - 3:
            stdscr.addstr(footer_r, 0, "├" + "─" * w + "┤")

            allow_cnt = sum(1 for i in q if is_allowed(i))
            block_cnt = sum(1 for i in q if not is_allowed(i))
            cache_cnt = sum(1 for i in q if i.get('cached'))
            live_cnt = sum(1 for i in q if not i.get('cached'))
            
            ftr1_content = f" Allow {allow_cnt} │ Block {block_cnt} │ Cache {cache_cnt} │ Live {live_cnt} │ {ups_str} │ WS Stream │ q Quit"
            stdscr.addstr(footer_r + 1, 0, f"│{ftr1_content:<{w}}│")
            
            ros_mem = router_cache['free_mem']
            ros_freq = router_cache['cpu_freq']
            ros_load = router_cache['cpu_load']
            ros_cnt_mem = router_cache['container_mem']
            
            ftr2_content = f" RouterOS: Free Mem: {ros_mem} │ CPU Freq: {ros_freq} │ CPU Load: {ros_load} │ Container Mem: {ros_cnt_mem}"
            stdscr.addstr(footer_r + 2, 0, f"│{ftr2_content:<{w}}│")

            try:
                stdscr.addstr(footer_r + 3, 0, "└" + "─" * w + "┘")
            except curses.error:
                pass

        stdscr.refresh()
        time.sleep(0.05)

if __name__ == "__main__":
    curses.wrapper(main)