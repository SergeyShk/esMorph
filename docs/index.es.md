# esMorph

**esMorph** es un analizador morfológico del español: un núcleo en Rust con API para Python. Para una forma de palabra busca los análisis en un diccionario - el lema, la categoría gramatical y los rasgos de [Universal Dependencies](https://universaldependencies.org/) ([Analizador](analyzer.md)). Las formas de un lexema según sus rasgos - conjugación, género y número - llegarán a continuación.

!!! warning "En desarrollo"
    Las palabras con clíticos, las desconocidas y la flexión llegarán en las próximas versiones. Las versiones `0.1.0.devN` se publican en TestPyPI.

## Instalación

Se requiere Python 3.11 o superior. Las versiones preliminares se publican solo en TestPyPI:

``` bash
pip install --index-url https://test.pypi.org/simple/ esmorph
```

Los paquetes precompilados y la compilación desde el código fuente se describen en la página de [instalación](installation.md).
