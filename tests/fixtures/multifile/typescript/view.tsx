import { foo } from "./lib";

export function View(): number {
  return foo(2); // .tsx calling into .ts: same link family
}
