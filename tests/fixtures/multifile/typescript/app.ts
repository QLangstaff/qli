import { foo } from "./lib";

function main(): void {
  foo(1);
  render(); // only a method named `render` exists: not a cross-file target
  missing();
}
