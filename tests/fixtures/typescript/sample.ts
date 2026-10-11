// TODO: replace the stub transport
const DEFAULT_RETRIES = 3;

/* FIXME: block comments count too */
function foo(x: number): number {
  // XXX: audit this arithmetic
  return x + 1;
}

class Widget {
  // HACK: temporary until the API settles
  render(): string {
    return "TODO this string must NOT be flagged";
  }
}

function main(): void {
  foo(DEFAULT_RETRIES);
  bar(); // bar is undefined in this file
}
