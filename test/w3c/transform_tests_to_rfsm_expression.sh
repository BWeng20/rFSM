#/bin/bash

SAXON_SOURCE_URL=https://raw.githubusercontent.com/Saxonica/Saxon-HE/main/12/Java/SaxonHE12-4J.zip
SAXON_JAR=saxon-he-12.4.jar
XSL_FILE=confRExp.xsl

echo "Started from $(pwd)"
SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"

cd $SCRIPT_DIR
echo "Working in $(pwd)"

abort_on_error() {
    echo "Failed!"
    exit 1
}

trap 'abort_on_error' ERR

if ! command -v java &> /dev/null
then
    echo "java not found, please install at least java 1.9!"
    exit 1
fi

if ! command -v xmllint &> /dev/null
then
    echo "xmllint not found, please install libxml2-utils!"
    exit 1
fi

if [ ! -f saxon/$SAXON_JAR ]; then
  # Try to download end unpack saxon open source version
  # For saxon and licencing (currently Mozilla Public License version 2.0) see
  # https://github.com/Saxonica/Saxon-HE

  if ! command -v unzip &> /dev/null
  then
      echo "unzip not found, please install it!"
      exit 1
  fi

  mkdir -p saxon
  cd saxon
  if [ ! -f Saxon4J.zip ]; then
    curl -o Saxon4J.zip $SAXON_SOURCE_URL
  fi
  unzip -n Saxon4J.zip

  if [ ! -f $SAXON_JAR ]; then
    echo "Error: The Saxon archive from '$SAXON_SOURCE_URL' does not contain the expected file '$SAXON_JAR'."
    exit 1
  fi
  cd ..
fi

mkdir -p manual_rfsm_expr
mkdir -p rfsm_expr
mkdir -p dependencies/rfsm_expr

# Select all mandatory, not-manual txml-test-files.
for TEST_URI in $(xmllint --xpath "//assert/test[@conformance='mandatory' and @manual='false']/start[contains(@uri,'.txml')]/@uri"  txml/manifest.xml | cut '-d"' -f2); do
  TEST_FILE=$(cut '-d/' -f2 <<< "$TEST_URI")
  if [ ! -f rfsm_expr/$TEST_FILE.scxml ]; then
    echo xsl processing $TEST_FILE
    java -jar saxon/$SAXON_JAR -o:rfsm_expr/$TEST_FILE.scxml -xsl:$XSL_FILE -s:txml/$TEST_FILE
  fi
done

# Select all mandatory, manual txml-test-files.
for TEST_URI in $(xmllint --xpath "//assert/test[@conformance='mandatory' and @manual='true']/start[contains(@uri,'.txml')]/@uri"  txml/manifest.xml | cut '-d"' -f2); do
  TEST_FILE=$(cut '-d/' -f2 <<< "$TEST_URI")
  if [ ! -f manual_rfsm_expr/$TEST_FILE.scxml ]; then
    echo xsl processing $TEST_FILE
    java -jar saxon/$SAXON_JAR -o:manual_rfsm_expr/$TEST_FILE.scxml -xsl:$XSL_FILE -s:manual_txml/$TEST_FILE
  fi
done


# Get all dependencies
for DEP_URI in $(xmllint --xpath "//assert/test[@conformance='mandatory']/dep/@uri"  txml/manifest.xml | cut '-d"' -f2); do
  DEP_FILE=$(cut '-d/' -f2 <<< "$DEP_URI")
  if [[ $DEP_FILE == *.txml ]]; then
    DEP_TARGET_FILE="${DEP_FILE%.txml}.scxml"
  else
    DEP_TARGET_FILE="${DEP_FILE}"
  fi

  if [[ $DEP_FILE == *.txml ]]; then
    if [ ! -f "dependencies/rfsm_expr/$DEP_TARGET_FILE" ]; then
      echo xsl processing $DEP_FILE to $DEP_TARGET_FILE
      java -jar saxon/$SAXON_JAR "-o:dependencies/rfsm_expr/$DEP_TARGET_FILE" -xsl:$XSL_FILE "-s:dependencies/$DEP_FILE"
    fi
  else
    cp "dependencies/$DEP_FILE" "dependencies/rfsm_expr/$DEP_FILE"
  fi
done


echo DONE
