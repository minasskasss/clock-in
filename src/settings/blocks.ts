/** One block being edited. `uid` is only a React key. */
export interface EditBlock {
  uid: string;
  id: string | null;
  start: string;
  end: string;
}

let nextUid = 0;

export function newUid(): string {
  nextUid += 1;
  return `b${nextUid}`;
}
