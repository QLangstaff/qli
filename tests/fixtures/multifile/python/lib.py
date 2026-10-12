"""Library half of the multi-file fixture: definitions other files call."""

SCALE = 2


def foo(x):
    return x * SCALE


class Gadget:
    def render(self):
        return foo(1)
