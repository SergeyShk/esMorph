# Análisis de palabras

Un análisis es un objeto `Analysis` que devuelven los métodos `analyze` y `analyze_many` del [analizador](analyzer.md): el lema, la categoría gramatical y los rasgos de [Universal Dependencies](https://universaldependencies.org/), la probabilidad, las palabras del token y la fuente del análisis. Los análisis son inmutables; se pueden comparar (la probabilidad no participa en la comparación, así que un análisis es igual a su forma de `forms()`), poner en conjuntos y serializar con pickle. Las formas de una palabra las dan los métodos `forms` e `inflect` - véase [Flexión](inflection.md).

=== "Código"

    ``` python
    from esmorph import Analyzer

    morph = Analyzer()
    analysis = morph.analyze("cantaban")[0]
    analysis.word, analysis.lemma, analysis.pos, analysis.feats, analysis.words, analysis.source
    ```

=== "Resultado"

    ```
    ('cantaban',
     'cantar',
     'VERB',
     {'Mood': 'Ind', 'Number': 'Plur', 'Person': '3', 'Tense': 'Imp', 'VerbForm': 'Fin'},
     ('cantaban',),
     'dictionary')
    ```

## Atributos { #attributes }

| Atributo | Tipo | Descripción |
| :------- | :--- | :---------- |
| `word` | `str` | La forma de palabra tal como la escribe el diccionario, con sus tildes; para una [grafía coloquial](#spelling), su grafía estándar (`trabajado` de `trabajao`); un verbo con [clíticos](#clitics) tal como está escrito, en minúsculas; un token de las [reglas](#rules) tal como está escrito, sin caracteres invisibles, con las marcas combinantes unidas a sus letras y con `-` en lugar de los guiones U+2010 y U+2011 |
| `lemma` | `str` | El lema |
| `pos` | `str` | Categoría gramatical de UD: `NOUN`, `VERB`, `ADJ`, `DET`, `ADP`, etc. |
| `feats` | `dict[str, str]` | Rasgos de UD: `{"Gender": "Fem", "Number": "Plur"}`; un diccionario nuevo cada vez, cambiarlo no cambia el análisis |
| `prob` | `float` | Parte del análisis entre los análisis de la palabra; la suma de todos es 1. En [`analyze_sentence`](analyzer.md#analyze_sentence) - la puntuación del modelo de contexto en la oración |
| `words` | `tuple[str, ...]` | Palabras del token: un verbo y sus clíticos, las palabras de una contracción (`del`, `al`, la antigua `desta`), un número y su signo o unidad (`60%`), una unidad y su número (`km.24`) o la propia palabra; el análisis describe la primera |
| `source` | `str` | De dónde viene el análisis: `"dictionary"` - la forma está en el diccionario, `"clitics"` - un verbo del diccionario con [clíticos](#clitics), `"rule"` - un [análisis por regla](#rules), `"guess"` - un [análisis estimado](#guess) de una palabra desconocida |
| `diminutive_of` | `str \| None` | La palabra de la que el lema es un [diminutivo](#diminutives): `casa` para `casitas`; `None` para las demás palabras |

## Categorías y rasgos { #ud }

Las categorías gramaticales, los rasgos y los lemas siguen las directrices de [Universal Dependencies](https://universaldependencies.org/u/pos/) y, donde estas callan, las convenciones del mayor treebank del español, [UD Spanish AnCora](https://github.com/UniversalDependencies/UD_Spanish-AnCora), así que el resultado se puede comparar con spaCy, Stanza y otras herramientas de UD:

- los determinantes, los pronombres, las preposiciones y las conjunciones tienen los lemas y los rasgos de AnCora: `la` es el artículo `el` (`Definite=Def`, `PronType=Art`) o el pronombre `él`, `se` es `él` con `Reflex=Yes`, `su` es `DET` con `Poss=Yes`, `muy` es una forma de `mucho`, y lo mismo sus grafías antiguas y orales, como variantes: `usté` es una forma de `tú`; un pronombre demostrativo con tilde (`éste`) es una variante del que no la lleva;
- `ser`, `estar`, `haber`, `poder`, `deber`, `soler`, `querer` y `saber` tienen análisis `AUX` y `VERB`; `hay` es ante todo el verbo `haber`;
- un participio es un verbo con el infinitivo como lema (`VerbForm=Part`, `Tense=Past`) y un adjetivo con el masculino singular como lema (`VerbForm=Part` sin `Tense`);
- los verbos no llevan `-se` en el lema: `quejarse` es `quejar` con el clítico `se`;
- las formas antiguas y no estándar de un verbo son sus variantes: `trujo` es `traer`, `vido` es `ver`, `habemos` es `haber`;
- el condicional y el imperativo no tienen `Tense`; un nombre propio no tiene rasgos, y su lema lleva mayúscula: `madrid` y `Madrid` son `Madrid`, `PSOE` sigue siendo `PSOE`;
- un adjetivo en `-ísimo` es su propio lema con `Degree=Abs`.

Si la categoría de una palabra depende del contexto, la palabra tiene ambos análisis: `que` es `PRON` y `SCONJ`, `la` es `DET` y `PRON`, un participio es `VERB` y `ADJ`. Rasgos: género, número, persona, tiempo, modo, forma verbal, grado, negación (`Polarity=Neg` de `no`), definitud (`Definite`), el tipo de pronombre o de determinante (`PronType`), la posesión (`Poss`), la reflexividad (`Reflex`), el caso de los pronombres personales (`Case`) y si van tras una preposición (`PrepCase`), la cortesía (`Polite=Form` de `usted`), el tipo de numeral (`NumType`) y cómo se escribe (`NumForm`), y la clase de un signo de puntuación (`PunctType`, `PunctSide`).

## Orden de los análisis { #order }

Los análisis van del más probable al menos probable. `prob` es la parte de un análisis entre los análisis de la palabra, según la frecuencia con que lo tienen los treebanks UD Spanish AnCora y GSD; de los verbos de una forma, el que tienen más a menudo en todas sus formas tiene la parte mayor donde la forma misma aparece poco: `vengo` es ante todo `venir`, no `vengar`, mientras que `sienta`, que tienen más a menudo como `sentar`, es ante todo `sentar`. Para una palabra que no han visto, las partes son iguales y el orden se estima por la terminación de la palabra y los rasgos más comunes; una forma de varios verbos que han visto en otras formas recibe en cambio partes según la frecuencia de los verbos y de los rasgos (`vengamos` es ante todo el subjuntivo de `venir`). Una palabra va antes que su grafía antigua con otras tildes: `á` es ante todo `a`, `tambien` es `también`. [`analyze_sentence`](analyzer.md#analyze_sentence) ordena los mismos análisis por la oración.

=== "Código"

    ``` python
    [(a.lemma, a.pos, round(a.prob, 2)) for a in morph.analyze("casa")][:2]
    ```

=== "Resultado"

    ```
    [('casa', 'NOUN', 0.99), ('casar', 'VERB', 0.01)]
    ```

## Diminutivos { #diminutives }

Un diminutivo conserva su propio lema, como en los treebanks: `casitas` es una forma de `casita`. `diminutive_of` da la palabra de la que es diminutivo, tal como la da Wikcionario en la primera acepción de la palabra: `casita` es diminutivo de `casa`, `poquito` de `poco`, `ahorita` de `ahora`. Una palabra que es sobre todo una palabra propia, aunque Wikcionario también la tenga como diminutivo, no lo tiene: `señorita`, `granito`. Lo tienen unos dos mil sustantivos, adjetivos y adverbios del diccionario.

=== "Código"

    ``` python
    [
        morph.analyze(word)[0].diminutive_of
        for word in ("casitas", "poquito", "ahorita", "señorita", "granito")
    ]
    ```

=== "Resultado"

    ```
    ['casa', 'poco', 'ahora', None, None]
    ```

## Palabras con clíticos { #clitics }

Un verbo con pronombres enclíticos se analiza como su verbo: el análisis describe el infinitivo, el gerundio o el imperativo, y `words` da las palabras del token, como las tiene Universal Dependencies. En la escritura antigua y literaria una forma personal también lleva enclíticos, si la tilde del token la marca: `díjole` es `dijo` + `le`. Desde 2010 un token así sigue las reglas generales de acentuación, así que una forma aguda pierde su tilde: una palabra en minúsculas que no es palabra del diccionario y es una forma así con un enclítico es ante todo ese verbo, con las conjeturas de la palabra después (`quedose` es `quedó` + `se`, `fuime` es `fui` + `me`), y [`is_known`](analyzer.md#is_known) no la reconoce. `del` y `al` también tienen sus dos palabras, como las contracciones de `de` con un pronombre o un demostrativo en la escritura antigua: `desta` es `de` + `esta`. Una palabra del diccionario no se divide: `comete` es una forma de `cometer`, aunque unas pocas suelen ser verbos con clíticos: `verse` es ante todo `ver` + `se`, `cállate` es `calla` + `te`, y en una oración siempre son esos verbos.

=== "Código"

    ``` python
    [(a.lemma, a.pos, a.words, a.source) for a in morph.analyze("dámelo")]
    ```

=== "Resultado"

    ```
    [('dar', 'VERB', ('da', 'me', 'lo'), 'clitics')]
    ```

## Ortografía { #spelling }

Las mayúsculas no importan para buscar una palabra en el diccionario, aunque un token con mayúscula también puede ser un [nombre](#rules). Una palabra escrita como en el diccionario da los análisis de esa grafía: `publico` no es `público`. Una palabra que no está tal como se escribe puede ser un verbo con clíticos (`verte` es `ver` + `te`, no `verté`); si no lo es, se busca sin sus tildes y con la diéresis perdida, y `word` muestra la grafía del diccionario; para un verbo con clíticos `word` es el token tal como se escribe, en minúsculas.

=== "Código"

    ``` python
    [morph.analyze(word)[0].word for word in ["CAMIÓN", "camion", "pinguino"]]
    ```

=== "Resultado"

    ```
    ['camión', 'camión', 'pingüino']
    ```

Una palabra en minúsculas que el diccionario no tiene, ni siquiera sin sus tildes, puede ser una grafía coloquial de una palabra del diccionario. Una palabra así recibe los análisis de esa palabra, y `word` es su grafía estándar. Las grafías son:

- un participio sin su `d`: `trabajao` es `trabajado`, `sío` es `sido`; su tilde no es una tilde perdida, así que `perdío` es `perdido`, no `perdió`;
- una `-á`, `-é` o `-ú` final en lugar de `-ad`, `-ed` o `-ud`: `caridá` es `caridad`, `virtú` es `virtud`;
- una `f` o una `h` aspiradas: `juerza` es `fuerza`, `juevo` es `huevo`, `juyendo` es `huyendo`.

Una palabra con mayúscula no es una grafía así: `Trabajao` y `Julita` son nombres, no `trabajado` y `fulita`.

El diccionario tiene además grafías de palabras tal como suenan y formas abreviadas comunes: `na` y `naa` son `nada`, `pos` es `pues`, `to` es `todo`, `mu` es `muy`, `güeno` es `bueno`, `er` es `el`. Tienen los análisis de la palabra estándar con su lema y sus rasgos, y `word` es la grafía estándar; el contexto elige entre ellos y las otras palabras de la misma grafía (`mu` también es una interjección). Como palabras del diccionario, se encuentran con cualquier uso de mayúsculas: `Güeno` también es `bueno`. Los análisis de todas estas grafías tienen `source` igual a `"dictionary"`, mientras que [`is_known`](analyzer.md#is_known) solo reconoce las grafías del diccionario: `na` es conocida, `trabajao` no.

=== "Código"

    ``` python
    [
        (analysis.word, analysis.lemma)
        for analysis in (
            morph.analyze(word)[0] for word in ["trabajao", "perdío", "caridá", "juerza", "na", "er"]
        )
    ]
    ```

=== "Resultado"

    ```
    [('trabajado', 'trabajar'), ('perdido', 'perder'), ('caridad', 'caridad'), ('fuerza', 'fuerza'), ('nada', 'nada'), ('el', 'el')]
    ```

## Tokens fuera del diccionario { #rules }

Un token que el diccionario no tiene recibe los análisis de unas reglas, con `source` igual a `"rule"` y `word` tal como está escrito:

- un signo de puntuación es `PUNCT` con el `PunctType` de sus signos si son de una misma clase, y `PunctSide` para los paréntesis, los corchetes, las llaves y los signos de interrogación y de exclamación; un símbolo como `%`, `€` o `²` es `SYM`;
- un número en cifras es `NUM` con `NumForm=Digit|NumType=Card`, también un año, también con signo, fracción, potencia o espacios entre los millares (`-5`, `37½`, `10²`, `10 000`); el lema quita los puntos entre los millares (`10.000` es `10000`); un número con signo de porcentaje, signo de grado o unidad son dos palabras, y el análisis es el del número (`60%` es `60` y `%`, `500cc` es `500` y `cc`, `-5ºC` es `-5` y `ºC`); una unidad de una letra lo es solo tal como se escribe (`3G` es un nombre), una más larga con cualquier uso de mayúsculas (`5Kg`, `8GB`);
- un número ordinal en cifras es `ADJ` con `NumForm=Digit|NumType=Ord`, su género y su número, y el masculino singular como lema: `2.ª` es `2.º`;
- un número romano es `NUM` con `NumForm=Roman`, con menos frecuencia un nombre; en minúsculas es solo un numeral: `xii`;
- un token con una letra mayúscula es un nombre, `PROPN`, con él mismo como lema: `Josep`, `CiU`, `W.`, `TV-3`;
- un enlace, un correo, una etiqueta o una mención en minúsculas es `SYM`: `www.aeat.es`, `#hola`;
- una abreviatura con punto en minúsculas que el diccionario no tiene es un sustantivo con ella misma como lema (`fig.`), y lo mismo una letra minúscula (`b`).

Una abreviatura con punto en minúsculas que el diccionario tiene sin el punto recibe los sustantivos y los símbolos de esa palabra, con `source` igual a `"dictionary"`: `kg.` es `kg`. Una unidad con su punto y un número en un solo token son dos palabras con estos análisis: `km.24` es `km` con las palabras `km.` y `24`, y solo cuentan sus sustantivos y símbolos (`m.36` no es un adjetivo). Una palabra del diccionario de otra categoría con un punto no recibe análisis: `come.` es un verbo con el punto de una oración.

Una palabra con mayúscula que el diccionario conoce también puede ser un nombre, con su parte. Una inicial (`D.`) y una palabra sin tildes que el diccionario solo tiene con ellas (`Angel`, `Perez`) suelen ser nombres, así que su nombre va primero, mientras que otra palabra con mayúscula es un nombre con menos frecuencia (`Estados`, `Comisión`), así que su nombre suele ir después de los análisis del diccionario. De las palabras en mayúsculas, una palabra de hasta cuatro letras suele ser una sigla (`ETA`, `PP`), así que su nombre suele ir primero, mientras que una palabra más larga y una palabra funcional - una preposición, un determinante, un pronombre, una conjunción, un auxiliar - suelen ser palabras de un titular (`CAMIONES`, `EL`), así que su nombre suele ir después de los análisis del diccionario. Un número romano que el diccionario conoce como palabra, como `VI`, `XI` o `X`, es ante todo un numeral. Un token con mayúscula que se divide en un verbo con clíticos también puede ser un nombre: un imperativo sin tilde suele serlo (`Mandela` es ante todo `PROPN`), y también una forma personal sin tilde (`Vila`), mientras que un infinitivo, un gerundio, un imperativo con tilde y una forma personal suelen ser verbos (`Hacerlo`, `Déjame`, `Levantóse`). Una palabra en minúsculas rara vez es un nombre: `miró` es ante todo `mirar`, `Miró` es un nombre. Un análisis de una regla no tiene otras formas: `forms()` da el propio análisis. `is_known` solo indica las palabras del diccionario.

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

- una palabra con guion se analiza por su última parte de una clase abierta o un numeral: `palestino-israelíes` es una forma de `palestino-israelí`;
- una palabra con un prefijo conocido se analiza por la palabra del diccionario sin él, de cuatro letras al menos tras un prefijo de tres: `supergallos` es una forma de `supergallo`, `sobreandan` de `sobreandar`;
- un verbo con clíticos se analiza por su gerundio estimado, o por su infinitivo estimado ante `se` o un clítico de tercera persona: `autogenerarse` es `autogenerar` + `se`;
- cualquier otra palabra se analiza por las palabras del diccionario con la misma terminación de hasta cinco letras: sus categorías, rasgos y flexión, según cuántas palabras los tienen y con qué frecuencia AnCora train tiene sus rasgos; una palabra en `-ísimo` es un adjetivo de `Degree=Abs`: `esbeltísima` es una forma de `esbeltísimo`.

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
