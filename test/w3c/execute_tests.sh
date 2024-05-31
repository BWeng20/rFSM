#/bin/bash

echo "Started from $(pwd)"
SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"
cd $SCRIPT_DIR
echo "Working in $(pwd)"

RFSM_BIN=../../target/debug/test

echo "======================================================="

for TEST_FILE in scxml/*.scxml; do
  echo "Testing $TEST_FILE"
  $RFSM_BIN $TEST_FILE
done

echo "======================================================="
echo DONE
