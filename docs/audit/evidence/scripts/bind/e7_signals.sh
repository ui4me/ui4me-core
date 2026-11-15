#!/bin/bash
. ./env.sh
echo "=== host SIGKILL"
python3 e7_exit.py hang > /dev/null 2>e7k.err &
LP=$!
sleep 3
HP=$(pgrep -P $LP | head -1)
echo "launcher $LP host $HP ($(cat /proc/$HP/comm))"
kill -9 $HP
wait $LP; echo "launcher exit=$?"
sleep 1; pgrep -fa "e7_exit.py hang" || echo "no leftover backend"
echo "=== host SIGSEGV-like (SIGABRT)"
python3 e7_exit.py hang > /dev/null 2>e7a.err &
LP=$!
sleep 3
HP=$(pgrep -P $LP | head -1)
kill -ABRT $HP
wait $LP; echo "launcher exit=$?"
sleep 1; pgrep -fa "e7_exit.py hang" || echo "no leftover backend"
echo "=== launcher SIGTERM"
python3 e7_exit.py hang > /dev/null 2>e7t.err &
LP=$!
sleep 3
kill -TERM $LP
wait $LP; echo "launcher exit=$?"
sleep 1; pgrep -fa "e7_exit.py hang" || echo "no leftover backend"
echo "=== launcher SIGKILL"
python3 e7_exit.py hang > /dev/null 2>e7l.err &
LP=$!
sleep 3
kill -KILL $LP
wait $LP 2>/dev/null; echo "launcher exit=$?"
sleep 2; pgrep -fa "e7_exit.py hang|ui4me-start-" || echo "no leftover host/backend"
