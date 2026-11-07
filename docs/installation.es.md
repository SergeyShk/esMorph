# Instalación

Se requiere Python 3.11 o superior. Las versiones preliminares `0.1.0.devN` se publican solo en TestPyPI:

``` bash
pip install --index-url https://test.pypi.org/simple/ esmorph
```

## Paquetes precompilados

Hay paquetes precompilados para Linux (x86_64 y aarch64, manylinux y musllinux), macOS (Intel y Apple Silicon) y Windows (x64 y arm64). Un mismo paquete sirve para todas las versiones de Python a partir de la 3.11, y Python 3.14t sin GIL tiene el suyo propio; para instalarlos no hace falta Rust. Python 3.13 sin GIL no es compatible: las compilaciones sin GIL empiezan en la 3.14.

## Compilación desde el código fuente

En otras plataformas y en las compilaciones sin GIL posteriores a la 3.14, pip compila el paquete desde el código fuente, lo que requiere [Rust](https://www.rust-lang.org/tools/install) 1.88 o superior:

``` bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
pip install 'maturin>=1.9.4,<2'
pip install --no-build-isolation --index-url https://test.pypi.org/simple/ esmorph
```

La dependencia de compilación se instala primero y desde PyPI, porque TestPyPI no tiene una versión adecuada de maturin; el paquete en sí debe venir solo de TestPyPI. Ejecute ambos comandos en un entorno virtual activado: con `--no-build-isolation`, la compilación busca el comando `maturin` en el `PATH`.

El entorno de desarrollo se describe en [CONTRIBUTING.md](https://github.com/SergeyShk/esMorph/blob/master/CONTRIBUTING.md).
