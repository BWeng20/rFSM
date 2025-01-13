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

mkdir -p manual_txml
mkdir -p manual_scxml
mkdir -p optional_txml
mkdir -p optional_scxml
mkdir -p txml
mkdir -p scxml
mkdir -p dependencies/scxml

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

# Select all mandatory, manual txml-test-files.
for TEST_URI in $(xmllint --xpath "//assert/test[@conformance='mandatory' and @manual='true']/start[contains(@uri,'.txml')]/@uri"  txml/manifest.xml | cut '-d"' -f2); do
  TEST_FILE=$(cut '-d/' -f2 <<< "$TEST_URI")
  if [ ! -f manual_txml/$TEST_FILE ]; then
    if [ -f manual_scxml/$TEST_FILE.scxml ]; then
      # Remove transformed version to force update
      rm manual_scxml/$TEST_FILE.scxml
    fi
    echo Fetching $TEST_SOURCE_URL$TEST_URI
    curl -o manual_txml/$TEST_FILE $TEST_SOURCE_URL$TEST_URI
  fi

  if [ ! -f manual_scxml/$TEST_FILE.scxml ]; then
    echo xsl processing $TEST_FILE
    java -jar saxon/$SAXON_JAR -o:manual_scxml/$TEST_FILE.scxml -xsl:txml/$XSL_FILE -s:manual_txml/$TEST_FILE
  fi
done

# Select all optional
for TEST_URI in $(xmllint --xpath "//assert/test[@conformance='optional']/start[contains(@uri,'.txml')]/@uri"  txml/manifest.xml | cut '-d"' -f2); do
  TEST_FILE=$(cut '-d/' -f2 <<< "$TEST_URI")
  if [ ! -f optional_txml/$TEST_FILE ]; then
    if [ -f optional_scxml/$TEST_FILE.scxml ]; then
      # Remove transformed version to force update
      rm optional_scxml/$TEST_FILE.scxml
    fi
    echo Fetching $TEST_SOURCE_URL$TEST_URI
    curl -o optional_txml/$TEST_FILE $TEST_SOURCE_URL$TEST_URI
  fi

  if [ ! -f optional_scxml/$TEST_FILE.scxml ]; then
    echo xsl processing $TEST_FILE
    java -jar saxon/$SAXON_JAR -o:optional_scxml/$TEST_FILE.scxml -xsl:txml/$XSL_FILE -s:optional_txml/$TEST_FILE
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

  if [ ! -f "dependencies/$DEP_FILE" ]; then
    rm "dependencies/scxml/$DEP_TARGET_FILE"
    echo Fetching $TEST_SOURCE_URL$DEP_URI
    curl -o "dependencies/$DEP_FILE" "$TEST_SOURCE_URL$DEP_URI"
  fi

  if [[ $DEP_FILE == *.txml ]]; then
    if [ ! -f "dependencies/scxml/$DEP_TARGET_FILE" ]; then
      echo xsl processing $DEP_FILE to $DEP_TARGET_FILE
      java -jar saxon/$SAXON_JAR "-o:dependencies/scxml/$DEP_TARGET_FILE" -xsl:txml/$XSL_FILE "-s:dependencies/$DEP_FILE"
    fi
  else
    cp "dependencies/$DEP_FILE" "dependencies/scxml/$DEP_FILE"
  fi
done


echo DONE
