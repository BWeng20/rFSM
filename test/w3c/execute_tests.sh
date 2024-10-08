#/bin/bash

echo "Started from $(pwd)"
SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"
cd $SCRIPT_DIR
echo "Working in $(pwd)"

if ! command -v xmllint &> /dev/null
then
    echo "xmllint not found, please install libxml2-utils!"
    exit 1
fi

RFSM_BIN=../../target/release/test

echo "======================================================="

export RUST_LOG=debug
export RUST_BACKTRACE=full

OK_COUNT=0
ALL_COUNT=0

if [ -d logs ]; then
  echo "Cleaning logs"
  rm -rf logs/*
fi
mkdir -p logs

REPORT_FILE="REPORT.MD"

echo "Write report to $REPORT_FILE"

echo "| Test                 | Result   | Test Description                                        |" > $REPORT_FILE
echo "|----------------------|----------|---------------------------------------------------------|" >> $REPORT_FILE

for TEST_FILE in scxml/*.scxml; do
  TEST_NAME=$(basename "${TEST_FILE}")

  # Extract comment
  TEST_COMMENT=$(xmllint --xpath "/comment()" "${TEST_FILE}" 2>/dev/null)
  TEST_COMMENT=${TEST_COMMENT//$'\n'/ }
  TEST_COMMENT=${TEST_COMMENT/\<!--/}
  TEST_COMMENT=${TEST_COMMENT/--\>/}
  TEST_COMMENT=${TEST_COMMENT//\</\\\<}

  TABLE_TEST_NAME="$TEST_NAME"
  if [ ${#TEST_NAME} -lt 21 ]; then
      TABLE_TEST_NAME="${TEST_NAME}                     "
      TABLE_TEST_NAME="${TABLE_TEST_NAME:0:21}"
  else
      TABLE_TEST_NAME="$TEST_NAME"
  fi
  echo -n "Testing ${TABLE_TEST_NAME} "
  echo -n "| ${TABLE_TEST_NAME}| " >> $REPORT_FILE

  $RFSM_BIN -includePaths dependencies/scxml test_config.json "$TEST_FILE" 1>"logs/$TEST_NAME.log" 2>&1
  if [ $? -eq 0 ]; then
      OK_COUNT=$(( OK_COUNT + 1 ))
      echo -e "\033[0;32mOK\033[0m"
      echo -n "OK       |" >> $REPORT_FILE
  else
      echo -e "\033[0;31mFailed\033[0m"
      echo -n "_Failed_ |" >> $REPORT_FILE
  fi
  echo "${TEST_COMMENT}|" >> $REPORT_FILE
  ALL_COUNT=$(( ALL_COUNT + 1 ))
done

echo "======================================================="
echo -e "\n__Result__: ${OK_COUNT} of ${ALL_COUNT} tests succeeded" >> $REPORT_FILE
echo "${OK_COUNT} of ${ALL_COUNT} tests succeeded"
