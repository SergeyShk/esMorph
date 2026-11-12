# Instalación

Se requiere Python 3.11 o superior.

``` bash
pip install esmorph
```

El paquete instala también el [comando `esmorph`](cli.md).

## Paquetes precompilados

PyPI tiene paquetes precompilados para Linux (x86_64 y aarch64, manylinux y musllinux), macOS (Intel y Apple Silicon) y Windows (x64 y arm64). Un mismo paquete sirve para todas las versiones de Python a partir de la 3.11, y Python 3.14t sin GIL tiene el suyo propio; para instalarlos no hace falta Rust. El diccionario forma parte del paquete y no requiere una instalación aparte. Python 3.13 sin GIL no es compatible: las compilaciones sin GIL empiezan en la 3.14.

Las versiones preliminares `X.Y.Z.devN` se publican solo en TestPyPI:

``` bash
pip install --index-url https://test.pypi.org/simple/ --extra-index-url https://pypi.org/simple/ --pre esmorph
```

uv toma un paquete del primer índice que lo tiene, así que necesita otro orden de índices:

``` bash
uv pip install --extra-index-url https://test.pypi.org/simple/ --index-strategy unsafe-best-match --pre esmorph
```

## Compilación desde el código fuente

En otras plataformas y en las compilaciones sin GIL posteriores a la 3.14, pip compila el paquete desde el código fuente, lo que requiere [Rust](https://www.rust-lang.org/tools/install) 1.88 o superior:

``` bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
pip install esmorph
```

El entorno de desarrollo se describe en [CONTRIBUTING.md](https://github.com/SergeyShk/esMorph/blob/master/CONTRIBUTING.md).

## Rust { #rust }

El núcleo del analizador se publica en crates.io como el crate [`esmorph`](https://crates.io/crates/esmorph), y el diccionario como el crate [`esmorph-dict-es`](https://crates.io/crates/esmorph-dict-es): se activa por defecto con la característica `bundled` y se carga con `Dictionary::bundled()`. La documentación de la API está en [docs.rs](https://docs.rs/esmorph).

``` bash
cargo add esmorph
```

La característica `anyseg` añade `Dictionary::annotate` para los documentos del segmentador [anySeg](https://sergeyshk.github.io/anySeg/), y la característica `cli` añade el [comando `esmorph`](cli.md), que se instala sin Python:

``` bash
cargo install esmorph --features cli
```
