# Instalación

Se requiere Python 3.11 o superior. Las versiones preliminares `0.1.0.devN` se publican solo en TestPyPI:

``` bash
pip install --index-url https://test.pypi.org/simple/ esmorph
```

## Paquetes precompilados

Hay paquetes precompilados para Linux (x86_64 y aarch64, manylinux y musllinux), macOS (Intel y Apple Silicon) y Windows (x64 y arm64). Un mismo paquete sirve para todas las versiones de Python a partir de la 3.11, y Python 3.14t sin GIL tiene el suyo propio; para instalarlos no hace falta Rust.

## Compilación desde el código fuente

En otras plataformas pip compila el paquete desde el código fuente, lo que requiere [Rust](https://www.rust-lang.org/tools/install) 1.88 o superior:

``` bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
pip install --index-url https://test.pypi.org/simple/ esmorph
```

El entorno de desarrollo se describe en [CONTRIBUTING.md](https://github.com/SergeyShk/esMorph/blob/master/CONTRIBUTING.md).
