. ./env.sh
setsid python3 e12_ctrlc.py > out12.stdout 2> out12.stderr &
sleep 3
PG=$(ps -o pgid= -p $! | tr -d ' ')
kill -INT -- -$PG
sleep 4
wait $! 2>/dev/null; 
