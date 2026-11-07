"""
Check of the installed package before publishing

Run as `python scripts/smoke.py [version]`; from the `scripts` directory the package is the installed
one, not the sources of the repository.
"""

import pickle
import sys
import sysconfig

import esmorph
from esmorph import Analyzer

morph = Analyzer()
analysis = morph.analyze("camiones")[0]
assert (analysis.lemma, analysis.pos) == ("camión", "NOUN"), analysis
assert analysis.feats == {"Gender": "Masc", "Number": "Plur"}, analysis
assert morph.analyze("camion")[0].word == "camión"
assert morph.is_known("hablamos")
assert morph.lemmatize(["Hablo", "qzxw"]) == ["hablar", "qzxw"]
assert pickle.loads(pickle.dumps(analysis)) == analysis
assert "site-packages" in esmorph.__file__, esmorph.__file__
# In Python without the GIL importing the module must not enable the GIL again
if sysconfig.get_config_var("Py_GIL_DISABLED"):
    assert not sys._is_gil_enabled()
if len(sys.argv) > 1:
    assert esmorph.__version__ == sys.argv[1], (esmorph.__version__, sys.argv[1])
print(f"esmorph {esmorph.__version__}: {morph.path}")
