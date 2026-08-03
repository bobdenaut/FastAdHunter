import curses, time, requests, urllib3, ipaddress, re, threading, os

urllib3.disable_warnings(urllib3.exceptions.InsecureRequestWarning)

TOKEN = "c48ac59cdaef2a10cf96ddeac66dd4d3c1be57bf1dcfbd9e96b0aed7de06ef65"
BASE = "https://172.17.0.2:8443"
H = {"Authorization": f"Bearer {TOKEN}"}

# Configurare RouterOS REST API
ROS_BASE = "https://192.168.10.1:8443/rest"
ROS_USER = "monitor"
ROS_PASS = os.getenv("MP", "")
ROS_AUTH = (ROS_USER, ROS_PASS)

ROUTER_FETCH_INTERVAL = 5

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

def fetch_mikrotik_stats_thread():
    global router_cache
    router_cache["is_loading"] = True
    try:
        # 1. Fetch /rest/system/resource
        r_res = requests.get(f"{ROS_BASE}/system/resource", auth=ROS_AUTH, verify=False, timeout=2)
        if r_res.status_code == 200:
            res_data = r_res.json()
            
            free_bytes = int(res_data.get("free-memory", 0))
            if free_bytes > 0:
                router_cache["free_mem"] = f"{free_bytes / 1024 / 1024:.1f}MiB"
            else:
                router_cache["free_mem"] = "N/A"

            freq = res_data.get("cpu-frequency", "N/A")
            router_cache["cpu_freq"] = f"{freq}MHz" if freq != "N/A" else "N/A"
            
            load = res_data.get("cpu-load", "N/A")
            router_cache["cpu_load"] = f"{load}%" if load != "N/A" else "N/A"

        # 2. Fetch /rest/container
        r_cnt = requests.get(f"{ROS_BASE}/container", auth=ROS_AUTH, verify=False, timeout=2)
        if r_cnt.status_code == 200:
            containers = r_cnt.json()
            if isinstance(containers, list):
                for item in containers:
                    if item.get("name") == "fastadhunter":
                        cnt_bytes = int(item.get("memory-current", 0))
                        if cnt_bytes > 0:
                            router_cache["container_mem"] = f"{cnt_bytes / 1024 / 1024:.1f}MiB"
                        else:
                            router_cache["container_mem"] = item.get("memory-current", "N/A")
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
        return str(ip.ipv4_mapped) if getattr(ip, "ipv4_mapped", None) else s
    except:
        return s

def bar(p, w=18):
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
            h_dots = int(((val - min_v) / (max_v - min_v)) * (total_dots_y - 1)) + 1
        
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
                if grid[gy][sub_x0]:
                    char_code |= dot_map[dy][0]
                if grid[gy][sub_x1]:
                    char_code |= dot_map[dy][1]
            
            row_str += chr(char_code)
        rows_text.append(row_str)

    return rows_text

def j(path):
    try:
        r = requests.get(BASE + path, headers=H, verify=False, timeout=2)
        if r.status_code == 200:
            return r.json(), True
        return {}, False
    except:
        return {}, False

def parse_metrics():
    try:
        r = requests.get(BASE + "/metrics", headers=H, verify=False, timeout=2)
        if r.status_code != 200:
            return {}, 0
        
        rules = 0
        upstreams = {}
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
        return upstreams, rules
    except:
        return {}, 0

def is_allowed(item):
    v = str(item.get("verdict", item.get("action", ""))).lower()
    return v in ["allow", "pass", "permitted"]

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

    while True:
        ch = stdscr.getch()
        if ch == ord('q') or ch == ord('Q'):
            stdscr.clear()
            stdscr.refresh()
            break

        update_router_stats_if_needed()

        stdscr.erase()

        max_y, max_x = stdscr.getmaxyx()
        w = max(100, max_x - 2)

        q_data, ok1 = j("/api/v1/queries")
        c_data, ok2 = j("/api/v1/cache")
        m_data, ok3 = j("/api/v1/debug/memory")
        upstreams, rules_count = parse_metrics()
        
        perf_pts = fetch_perf_history()
        
        is_online = ok1 and ok2 and ok3

        q = q_data.get("items", [])
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

        c1_rss   = f"  RSS   {bar(min(rss, 150) / 150 * 100)} {rss:5.1f} MB"
        c2_rss   = f"Peak {peak:5.1f} MB"
        c3_rss   = f"Ruleset {ruleset_mb:4.1f} MB │ Alloc Peak {alloc_peak_mb:5.1f} MB"

        c1_hit   = f"  Hit   {bar(hp)} {hp:5.1f}%"
        c2_hit   = f"{h}/{h + ms}"
        c3_hit   = ""

        c1_cache = f"  Cache {bar(load)} {load:5.1f}%"
        c2_cache = f"{c.get('entries', 0)}/{c.get('capacity', 0)} ({c_bytes:.1f}/{c_max_bytes:.0f} MB)"
        c3_cache = f"Fresh {fresh} │ Stale {stale}"

        left_part_l3 = f"│{c1_rss:<36}│ {c2_rss:<28}│ {c3_rss:<38}"
        left_part_l4 = f"│{c1_hit:<36}│ {c2_hit:<28}│ {c3_hit:<38}"
        left_part_l5 = f"│{c1_cache:<36}│ {c2_cache:<28}│ {c3_cache:<38}"

        graph_width = w - len(left_part_l3) - 1
        if graph_width < 5:
            graph_width = 0

        # Header
        t_str = time.strftime('%H:%M:%S')
        status_txt = "● ONLINE" if is_online else "● OFFLINE"
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

        lbl = " Live Queries "
        fill = w - len(lbl)
        l_fill = fill // 2
        r_fill = fill - l_fill
        stdscr.addstr(6, 0, "├" + "─" * l_fill + lbl + "─" * r_fill + "┤")

        domain_width = max(20, w - 79)

        hdr_str = f"│  {'Client':<39} {'Domain':<{domain_width}} {'Type':<6} {'Verdict':<9} {'Cache':<6} {'Time':<7}"
        stdscr.addstr(7, 0, f"{hdr_str:<{w+1}}│")

        # MODIFICAT: max_y - 14 oferă loc curat celor 4 rânduri de footer
        max_rows = max(3, max_y - 14)
        
        r = 8
        for it in q[:max_rows]:
            client = fmtip(str(it.get('client', '')))[ :39]
            domain = str(it.get('domain', ''))[:domain_width]
            qtype = str(it.get('qtype', ''))[:6]
            
            allowed = is_allowed(it)
            v_char = "Pass" if allowed else "Block"
            
            if use_256:
                row_attr = curses.color_pair(3) if allowed else curses.color_pair(4)
            else:
                row_attr = curses.color_pair(1) if allowed else curses.color_pair(2)
            
            cache_str = "Yes" if it.get("cached") else "No"
            duration = f"{it.get('duration_ms', 0):7.3f}"

            stdscr.addstr(r, 0, "│")
            row_content = f"  {client:<39} {domain:<{domain_width}} {qtype:<6} {v_char:<9} {cache_str:<6} {duration:<7}"
            row_full = f"{row_content:<{w}}"
            
            stdscr.addstr(r, 1, row_full, row_attr)
            stdscr.addch(r, w + 1, '│')
            r += 1

        while r < 8 + max_rows:
            if r < max_y - 5:
                stdscr.addstr(r, 0, "│" + " " * w + "│")
            r += 1

        footer_r = 8 + max_rows
        stdscr.addstr(footer_r, 0, "├" + "─" * w + "┤")

        allow_cnt = sum(1 for i in q if is_allowed(i))
        block_cnt = sum(1 for i in q if not is_allowed(i))
        cache_cnt = sum(1 for i in q if i.get('cached'))
        live_cnt = sum(1 for i in q if not i.get('cached'))
        
        # Line 1 Footer - App stats (CORRECTED FORMATTING)
        ftr1_content = f" Allow {allow_cnt} │ Block {block_cnt} │ Cache {cache_cnt} │ Live {live_cnt} │ {ups_str} │ Refresh 1.0 s │ q Quit"
        stdscr.addstr(footer_r + 1, 0, f"│{ftr1_content:<{w}}│")

        # Line 2 Footer - Empty Spacer (CORRECTED FORMATTING)
        empty_content = ""
        stdscr.addstr(footer_r + 2, 0, f"│{empty_content:<{w}}│")
        
        # Line 3 Footer - RouterOS stats (CORRECTED FORMATTING)
        ros_mem = router_cache['free_mem']
        ros_freq = router_cache['cpu_freq']
        ros_load = router_cache['cpu_load']
        ros_cnt_mem = router_cache['container_mem']
        
        ftr2_content = f" RouterOS: Free Mem: {ros_mem} │ CPU Freq: {ros_freq} │ CPU Load: {ros_load} │ Container Mem: {ros_cnt_mem}"
        stdscr.addstr(footer_r + 3, 0, f"│{ftr2_content:<{w}}│")

        # Line 4 Footer - Bottom Border cu try/except
        try:
            stdscr.addstr(footer_r + 4, 0, "└" + "─" * w + "┘")
        except curses.error:
            pass

        stdscr.refresh()
        time.sleep(0.2)

if __name__ == "__main__":
    curses.wrapper(main)