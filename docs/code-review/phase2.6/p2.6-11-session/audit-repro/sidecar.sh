#!/bin/sh
set -u
apk add --no-cache strace >/dev/null 2>&1 || { echo "apk strace failed"; exit 1; }

PID=1
OUT=/out

smaps_loop() {
    prev=0
    while true; do
        ts=$(date +%s)
        rss=$(awk '/^Rss:/{print $2}' /proc/$PID/smaps_rollup 2>/dev/null)
        anon=$(awk '/^Anonymous:/{print $2}' /proc/$PID/smaps_rollup 2>/dev/null)
        [ -z "$rss" ] && { echo "$ts target gone" >> $OUT/smaps.log; break; }
        echo "$ts $rss $anon" >> $OUT/smaps.log
        if [ "$prev" -gt 0 ] && [ $((rss - prev)) -ge 4096 ]; then
            cp /proc/$PID/smaps "$OUT/smaps-full-$ts.txt" 2>/dev/null
            cp /proc/$PID/maps "$OUT/maps-$ts.txt" 2>/dev/null
            echo "$ts JUMP +$((rss - prev))kB dumped" >> $OUT/smaps.log
        fi
        prev=$rss
        sleep 1
    done
}

smaps_loop &
exec strace -f -tt -e trace=mmap,munmap,madvise,mprotect -p $PID -o $OUT/strace.log
