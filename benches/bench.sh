#!/bin/bash

BROWSER_HEADERS='
-H "Host: localhost"
-H "User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36"
-H "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
-H "Accept-Language: en-US,en;q=0.5"
-H "Accept-Encoding: gzip, deflate"
-H "Connection: keep-alive"
-H "Upgrade-Insecure-Requests: 1"
-H "Cache-Control: max-age=0"
-H "Sec-Fetch-Dest: document"
-H "Sec-Fetch-Mode: navigate"
-H "Sec-Fetch-Site: none"
-H "Sec-Fetch-User: ?1"
-H "DNT: 1"
'

TARGET_URL="http://localhost:8080/"
OUTPUT_FILE="./result/wrk_$1.txt"

touch $OUTPUT_FILE

THREADS_LIST=(1 2 3 4)
CONNECTIONS_LIST=(10 100 1000 2500 5000)
DURATION="30s"

ulimit -n 65535

echo "==========================================" > "$OUTPUT_FILE"
echo "           Benchmarks server" >> "$OUTPUT_FILE"
echo "==========================================" >> "$OUTPUT_FILE"
echo "Target server: $TARGET_URL" >> "$OUTPUT_FILE"
echo "Test machine: $(hostname) | $(uname -s -r -m)" >> "$OUTPUT_FILE"
echo "CPU: $(grep -m1 "model name" /proc/cpuinfo | cut -d: -f2 | sed 's/^[ \t]*//;s/[ \t]*$//')" >> "$OUTPUT_FILE"
echo "Memory: $(free -h | awk '/^Mem:/ {print $2}') RAM" >> "$OUTPUT_FILE"
echo "==========================================" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"

TEST_COUNT=0
TOTAL_TESTS=$(( ${#THREADS_LIST[@]} * ${#CONNECTIONS_LIST[@]} ))

for THREADS in "${THREADS_LIST[@]}"; do
    for CONNECTIONS in "${CONNECTIONS_LIST[@]}"; do
        TEST_COUNT=$((TEST_COUNT + 1))
        
        echo "Running test $TEST_COUNT/$TOTAL_TESTS: threads=$THREADS, conn=$CONNECTIONS"

        eval wrk -t$THREADS -c$CONNECTIONS -d$DURATION $BROWSER_HEADERS "$TARGET_URL" >> "$OUTPUT_FILE" 2>&1

        echo "" >> "$OUTPUT_FILE"
        
        sleep 2
    done
done

echo "==========================================" >> "$OUTPUT_FILE"
echo "Results saved to: $OUTPUT_FILE" >> "$OUTPUT_FILE"
echo "==========================================" >> "$OUTPUT_FILE"
