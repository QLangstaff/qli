// Library half of the multi-file fixture. `foo` is also defined in
// ../python/lib.py: a TypeScript call must never resolve to the Python one.
export function foo(x: number): number {
  return x + 1;
}

export class Gadget {
  render(): string {
    return "gadget";
  }
}
