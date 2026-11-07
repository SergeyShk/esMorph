<h1 align="center">esMorph</h1>

<p align="center">
  Analizador morfológico rápido del español escrito en Rust, con API para Python
</p>

<p align="center">
  <a href="https://github.com/SergeyShk/esMorph/blob/master/README.md">English</a>
</p>

<p align="center">
  <a href="https://github.com/SergeyShk/esMorph/actions/workflows/ci.yml"><img src="https://github.com/SergeyShk/esMorph/actions/workflows/ci.yml/badge.svg" alt="Build"></a>
  <a href="https://github.com/astral-sh/ruff"><img src="https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/astral-sh/ruff/main/assets/badge/v2.json" alt="Ruff"></a>
  <a href="https://github.com/SergeyShk/esMorph/blob/master/LICENSE.txt"><img src="https://img.shields.io/github/license/sergeyshk/esmorph.svg" alt="Licencia"></a>
</p>

---

**esMorph** es un analizador morfológico del español: un núcleo en Rust con API para Python. Para una forma de palabra buscará los análisis en un diccionario - el lema, la categoría gramatical y los rasgos de [Universal Dependencies](https://universaldependencies.org/) - y dará las formas de un lexema según sus rasgos: conjugación, género y número.

> [!WARNING]
> En desarrollo: por ahora el paquete solo contiene la estructura básica; el analizador llegará en las próximas versiones. Las versiones `0.1.0.devN` se publican en TestPyPI, y el sitio de la documentación aparecerá con el primer lanzamiento.

## Instalación

Se requiere Python 3.11 o superior. Las versiones preliminares se publican solo en TestPyPI:

```bash
pip install --index-url https://test.pypi.org/simple/ --extra-index-url https://pypi.org/simple/ esmorph
```

Hay paquetes precompilados para Linux (también Alpine), macOS y Windows y para Python 3.14t sin GIL; en otras plataformas la compilación desde el código fuente requiere [Rust](https://www.rust-lang.org/tools/install) 1.88 o superior. Más detalles en la página de [instalación](https://github.com/SergeyShk/esMorph/blob/master/docs/installation.es.md).

## Desarrollo

El proyecto usa [uv](https://docs.astral.sh/uv/) para gestionar las dependencias, [maturin](https://www.maturin.rs/) para compilar el módulo de Rust y [ruff](https://docs.astral.sh/ruff/) para el análisis y el formato del código. Se requieren Python 3.11 o superior y Rust 1.88 o superior.

```bash
git clone https://github.com/SergeyShk/esMorph.git
cd esMorph

make deps        # crear el entorno, instalar las dependencias y compilar el módulo de Rust
make test        # ejecutar las pruebas de Python, los ejemplos de los docstrings (doctest) y las pruebas de Rust
make lint        # ruff, mypy, rustfmt y clippy
```

La lista completa de comandos está en `make help`. La documentación es bilingüe: las páginas en inglés `docs/*.md` con las páginas en español `docs/*.es.md` al lado; al editar una página, actualice las dos versiones.

Antes de enviar cambios, instale los hooks que ejecutan los linters al hacer commit y las pruebas al hacer push:

```bash
uv run pre-commit install
```

## Contribuir

Los informes de errores, las ideas y los pull requests son bienvenidos, en inglés o en español - las [issues](https://github.com/SergeyShk/esMorph/issues) están abiertas. El flujo de trabajo y las comprobaciones previas a un pull request están en [CONTRIBUTING.md](https://github.com/SergeyShk/esMorph/blob/master/CONTRIBUTING.md).

<details>
<summary><b>Estructura del proyecto</b></summary>

<br>

*   **crates/esmorph** - el núcleo del analizador en Rust
*   **crates/esmorph-py** - el módulo de Python `esmorph._esmorph`, compilado con PyO3
*   **python/esmorph** - el paquete de Python
*   **docs** - la documentación de MkDocs en inglés y en español
*   **tests** - las pruebas del paquete de Python

</details>

## Autores

*   Sergey Shkarin (kouki.sergey@gmail.com)

## Licencia

El código se distribuye bajo la licencia [MIT](https://github.com/SergeyShk/esMorph/blob/master/LICENSE.txt).
