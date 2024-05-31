# Tests from W3C

This folder contains scripts to execute the tests described in [SCXML 1.0 Implementation Report](https://www.w3.org/Voice/2013/scxml-irp/).

## Download and Transform

The original tests are written in a data-model-agnostic way and 
need a xsl transformation to (in this case) the ECMA-data-model.<br/>
W3C delivers a xsl-transformation for this case.

To download and transform call the bash script `download_and_transform_tests.sh`

The script never downloads a file twice, to update to newer versions,
delete the folder `txml` and call `download_and_transform_tests.sh` again.

The Ids of the test are extracted from manifest.xml, included with the tests from W3C.
The script select all _mandatory_ and _automated_ txml-tests. Optional or manual tests are ignored.

The xsl seems (afaik) to be usable only with [SAXON](https://github.com/Saxonica/Saxon-HE).<br/>
The download script tries to download the open-source-version of SAXON
and call it to transform the W3C scripts. The transformed test are placed in the folder `scxml`.

### Requierments
+ __bash__ To execute the script. In MS-Windows you can use __wsl__.
+ __java__ (at least 1.9, check SAXON documention in case of issues)
+ __curl__ (to download SAXON and the test sources)
+ __unzip__ (to uncompress SAXON)
+ __xmllint__ (from `libxml2-utils`) to extract the matching tests from manifest.
+ Internet connection.

## Running the tests

TODO

## Current status

The following table gives the current test result for rFSM:

| Test | Conformance | Result |
|------|-------------|--------|
| 355  | mandatory   |        |
| 576  | mandatory   |        |
| 364  | mandatory   |        |
| 372  | mandatory   |        |
|      |             |        |
|      |             |        |
|      |             |        |
|      |             |        |
|      |             |        |


