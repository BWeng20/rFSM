#/bin/bash

TEST_SOURCE_URL=https://www.w3.org/Voice/2013/scxml-irp/
XSL_FILE=confEcma.xsl
MANIFEST_URL=https://www.w3.org/Voice/2013/scxml-irp/manifest.xml

SAXON_SOURCE_URL=https://raw.githubusercontent.com/Saxonica/Saxon-HE/main/12/Java/SaxonHE12-4J.zip
SAXON_JAR=saxon-he-12.4.jar

echo "Started from $(pwd)"
SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"

cd $SCRIPT_DIR
echo "Working in $(pwd)"

abort_on_error() {
    echo "Failed!"
    exit 1
}

trap 'abort_on_error' ERR

if ! command -v curl &> /dev/null
then
    echo "curl not found, please install it!"
    exit 1
fi

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

mkdir -p txml
mkdir -p scxml

if [ ! -f txml/$XSL_FILE ]; then
  echo $TEST_SOURCE_URL$XSL_FILE
  curl -o txml/$XSL_FILE $TEST_SOURCE_URL$XSL_FILE
fi


if [ ! -f txml/manifest.xml ]; then
  echo $MANIFEST_URL
  curl -o txml/manifest.xml $MANIFEST_URL
fi

# Select all mandatory, not-manual txml-test-files.
for TEST_URI in $(xmllint --xpath "//assert/test[@conformance='mandatory' and @manual='false']/start[contains(@uri,'.txml')]/@uri"  txml/manifest.xml | cut '-d"' -f2); do
  TEST_FILE=$(cut '-d/' -f2 <<< "$TEST_URI")
  if [ ! -f txml/$TEST_FILE ]; then
    if [ -f scxml/$TEST_FILE.scxml ]; then
      # Remove transformed version to force update
      rm scxml/$TEST_FILE.scxml
    fi
    echo Fetching $TEST_SOURCE_URL$TEST_URI
    curl -o txml/$TEST_FILE $TEST_SOURCE_URL$TEST_URI
  fi

  if [ ! -f scxml/$TEST_FILE.scxml ]; then
    echo xsl processing $TEST_FILE
    java -jar saxon/$SAXON_JAR -o:scxml/$TEST_FILE.scxml -xsl:txml/$XSL_FILE -s:txml/$TEST_FILE
  fi
done

echo DONE
