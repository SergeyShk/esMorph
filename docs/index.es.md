# esMorph

**esMorph** es un analizador morfológico del español: un núcleo en Rust con API para Python. Para una forma de palabra buscará los análisis en un diccionario - el lema, la categoría gramatical y los rasgos de [Universal Dependencies](https://universaldependencies.org/) - y dará las formas de un lexema según sus rasgos: conjugación, género y número.

!!! warning "En desarrollo"
    Por ahora el paquete solo contiene la estructura básica; el analizador llegará en las próximas versiones. Las versiones `0.1.0.devN` se publican en TestPyPI.

## Instalación

Se requiere Python 3.11 o superior. Las versiones preliminares se publican solo en TestPyPI:

``` bash
pip install --index-url https://test.pypi.org/simple/ --extra-index-url https://pypi.org/simple/ esmorph
```

Los paquetes precompilados y la compilación desde el código fuente se describen en la página de [instalación](installation.md).
