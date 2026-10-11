# TODO: wire up the real client
"""A sample module for qli's analyzer fixtures."""


# FIXME: this default is a guess
DEFAULT_RETRIES = 3


def foo(x):
    # XXX: revisit the off-by-one below
    return x + 1


class Widget:
    # HACK: temporary shim until the API stabilizes
    def render(self):
        return "TODO this string must NOT be flagged"


def main():
    foo(DEFAULT_RETRIES)
    bar()  # bar is undefined in this file
