#!/bin/bash

FOLDERS=( "hyper/" "maker_web/" ) 

if [ ! -f "bench.sh" ]; then
    cd "benches"
fi

chmod +x bench.sh

for folder in "${FOLDERS[@]}"; do
    pkill -f "bench_$SERVER_PID"
done

for folder in "${FOLDERS[@]}"; do
    pushd $folder
        ulimit -n 65535

        folder=${folder%/}

        cargo run --release & 
        SERVER_PID=$!

        while ! nc -z localhost 8080; do
            sleep 1
        done

        sleep 2

        ../bench.sh $folder
        
        for folder in "${FOLDERS[@]}"; do
            pkill -f "bench_$SERVER_PID"
        done
    popd
done
