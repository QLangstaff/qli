"""Application half: every call here targets a definition in another file."""
from helpers import helper
from lib import Gadget, foo


def main():
    foo(1)
    Gadget()
    naïve = "é"; foo(naïve)
    helper()
    missing()
