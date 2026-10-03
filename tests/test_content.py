import pytest
from bot.services.content import normalize,validate_safe_text

def test_unicode_normalization_and_excluded_content():
    assert normalize("  Sailor   MOON ")=="sailor moon"
    with pytest.raises(ValueError): validate_safe_text("explicit porn",maximum=100)

