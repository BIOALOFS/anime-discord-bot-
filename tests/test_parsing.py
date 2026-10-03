from types import SimpleNamespace
import pytest
from bot.commands.parsing import find_arguments,profile_arguments

def test_all_profile_prefix_forms():
    author=SimpleNamespace(id=1);other=SimpleNamespace(id=2)
    assert profile_arguments("",[],author)==(author,1)
    assert profile_arguments("5",[],author)==(author,5)
    assert profile_arguments("<@2>",[other],author)==(other,1)
    assert profile_arguments("<@2> 5",[other],author)==(other,5)
    with pytest.raises(ValueError):profile_arguments("person five",[],author)
    with pytest.raises(ValueError):profile_arguments("0",[],author)

def test_find_prefix_mention_and_unquoted_multiword_query():
    author=SimpleNamespace(id=1);other=SimpleNamespace(id=2)
    assert find_arguments("Sung Jinwoo",[],author)==(author,"Sung Jinwoo")
    assert find_arguments("<@2> Sung Jinwoo",[other],author)==(other,"Sung Jinwoo")
    with pytest.raises(ValueError):find_arguments("<@2>",[other],author)
