# Analizador

`Analyzer` busca los análisis de una forma de palabra en un diccionario creado a partir de Wiktionary: el lema, la categoría gramatical y los rasgos de [Universal Dependencies](https://universaldependencies.org/).

=== "Código"

    ``` python
    from esmorph import Analyzer

    morph = Analyzer()
    analysis = morph.analyze("camiones")[0]
    analysis.word, analysis.lemma, analysis.pos, analysis.feats
    ```

=== "Resultado"

    ```
    ('camiones', 'camión', 'NOUN', {'Gender': 'Masc', 'Number': 'Plur'})
    ```

## Orden de los análisis { #order }

Los análisis van del más probable al menos probable. `prob` es la parte de un análisis entre los análisis de la palabra, según la frecuencia con que lo tienen los treebanks UD Spanish AnCora y GSD; para una palabra que no han visto, las partes son iguales y el orden se estima por la terminación de la palabra y los rasgos más comunes.

=== "Código"

    ``` python
    [(a.lemma, a.pos, round(a.prob, 2)) for a in morph.analyze("casa")][:2]
    ```

=== "Resultado"

    ```
    [('casa', 'NOUN', 0.99), ('casar', 'VERB', 0.01)]
    ```

Los lemas y los rasgos siguen las convenciones de estos treebanks: `la` es una forma de `el`, `se` de `él`, `muy` de `mucho`, y los verbos no llevan `-se` en el lema. Un verbo auxiliar tiene análisis `AUX` y `VERB`, y un participio también es un adjetivo.

## Palabras con clíticos { #clitics }

Un verbo con pronombres enclíticos se analiza como su verbo: el análisis describe el infinitivo, el gerundio o el imperativo, y `words` da las palabras del token, como las tiene Universal Dependencies. `del` y `al` también tienen sus dos palabras. Una palabra del diccionario no se divide: `comete` es una forma de `cometer`.

=== "Código"

    ``` python
    [(a.lemma, a.pos, a.words, a.source) for a in morph.analyze("dámelo")]
    ```

=== "Resultado"

    ```
    [('dar', 'VERB', ('da', 'me', 'lo'), 'clitics')]
    ```

## Ortografía { #spelling }

Las mayúsculas no importan. Una palabra escrita como en el diccionario da los análisis de esa grafía: `publico` no es `público`. Una palabra que no está tal como se escribe se busca sin sus tildes y con la diéresis perdida, y `word` muestra la grafía del diccionario; para un verbo con clíticos `word` es el token tal como se escribe, en minúsculas.

=== "Código"

    ``` python
    [morph.analyze(word)[0].word for word in ["CAMIÓN", "camion", "pinguino"]]
    ```

=== "Resultado"

    ```
    ['camión', 'camión', 'pingüino']
    ```

## Tokens fuera del diccionario { #rules }

Un token que el diccionario no tiene recibe los análisis de unas reglas, con `source` igual a `"rule"` y `word` tal como está escrito:

- un signo de puntuación es `PUNCT` con su `PunctType`, y `PunctSide` para los signos dobles; un símbolo como `%`, `€` o `²` es `SYM`;
- un número en cifras es `NUM` con `NumForm=Digit|NumType=Card`, también un año; el lema quita los puntos entre los millares: `10.000` es `10000`;
- un número romano es `NUM` con `NumForm=Roman`, con menos frecuencia un nombre;
- un token con una letra mayúscula es un nombre, `PROPN`, con él mismo como lema: `Josep`, `CiU`, `W.`, `TV-3`.

Una palabra en mayúsculas que el diccionario conoce también puede ser un nombre, con su parte: una palabra de hasta cuatro letras suele ser una sigla (`ETA`, `PP`), así que su nombre suele ir primero, mientras que una palabra más larga y una palabra de una clase cerrada suelen ser palabras de un titular (`CAMIONES`, `EL`), así que su nombre suele ir después de los análisis del diccionario. Un número romano en `I` como `VI` o `XI` es ante todo un numeral. Un token con mayúscula que se divide en un verbo con clíticos también suele ser un nombre: `Mandela` es ante todo `PROPN`. Un análisis de una regla no tiene otras formas: `forms()` da el propio análisis. `is_known` solo indica las palabras del diccionario.

=== "Código"

    ``` python
    [(a.lemma, a.pos, a.feats, a.source) for a in morph.analyze("10.000")]
    [(a.lemma, a.pos, round(a.prob, 2)) for a in morph.analyze("ETA")]
    ```

=== "Resultado"

    ```
    [('10000', 'NUM', {'NumForm': 'Digit', 'NumType': 'Card'}, 'rule')]
    [('ETA', 'PROPN', 0.82), ('eta', 'NOUN', 0.18)]
    ```

## Palabras desconocidas { #guess }

Una palabra en minúsculas que ni el diccionario ni las reglas conocen recibe análisis estimados, con `source` igual a `"guess"`:

- una palabra con guion se analiza por su última parte de una clase abierta: `palestino-israelíes` es una forma de `palestino-israelí`;
- una palabra con un prefijo conocido se analiza por la palabra del diccionario sin él: `supergallos` es una forma de `supergallo`, `sobreandan` de `sobreandar`;
- un verbo con clíticos se analiza por su gerundio estimado, o por su infinitivo estimado ante `se` o un clítico de tercera persona: `autogenerarse` es `autogenerar` + `se`;
- cualquier otra palabra se analiza por las palabras del diccionario con la misma terminación de hasta cinco letras: sus categorías, rasgos y flexión, según cuántas palabras los tienen y con qué frecuencia los treebanks tienen sus rasgos.

Una palabra estimada tiene sus formas, también estimadas: `forms()` e `inflect()` funcionan como con una palabra del diccionario. `is_known` sigue siendo falso para ella.

=== "Código"

    ``` python
    [(a.lemma, a.pos, round(a.prob, 2)) for a in morph.analyze("zumbeábamos")]
    morph.analyze("zumbeábamos")[0].inflect(Tense="Pres", Person="1", Number="Sing")
    ```

=== "Resultado"

    ```
    [('zumbear', 'VERB', 0.9), ('zumbeábar', 'VERB', 0.09), ('zumbeábar', 'VERB', 0.01)]
    'zumbeo'
    ```

## Métodos { #methods }

| Método | Resultado |
| :-- | :-- |
| `Analyzer(path=None)` | Un analizador con el diccionario del paquete o de un archivo |
| `analyze(word)` | La lista de `Analysis`; vacía para un token que nada analiza, como `qzxw` |
| `analyze_many(words)` | Lo mismo para una colección de palabras, sin el GIL |
| `lemmatize(words)` | El lema del primer análisis de cada palabra; una palabra sin análisis en minúsculas, sin el GIL |
| `is_known(word)` | Si el diccionario tiene la palabra, tal como está escrita o como un verbo con clíticos |

`Analysis` tiene los campos `word`, `lemma`, `pos`, `feats`, `prob`, `words` y `source` y los métodos `forms()` e `inflect()` - véase [Flexión](inflection.md). Es inmutable, se puede usar como clave y se compara sin la probabilidad. El analizador y el análisis se pueden serializar con pickle: al deserializarlos se toma el analizador del mismo diccionario que el proceso ya tiene, así que los análisis deserializados uno a uno lo comparten, y un análisis cuyo archivo de diccionario ya no existe toma el diccionario del paquete.
