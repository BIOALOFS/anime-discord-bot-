import re
import unicodedata

BLOCKED = re.compile(r"\b(?:porn|hentai|explicit|gore|18\+|nsfw)\b|(?:إباح|جنسي صريح|دموي)", re.I)


def normalize(value: str) -> str:
    return " ".join("".join(c for c in unicodedata.normalize("NFKC", value).casefold() if not unicodedata.combining(c)).split())


def validate_safe_text(value: str, *, maximum: int, field: str = "text") -> str:
    value = value.strip()
    if not value or len(value) > maximum:
        raise ValueError(f"{field} must contain 1-{maximum} characters")
    if BLOCKED.search(value):
        raise ValueError(f"{field} contains excluded content and requires review")
    return value

