import assert from "node:assert/strict";
import { it } from "node:test";

import { main } from "./run-long-validation.mjs";

it("runs an explicitly classified long-validation command under an external deadline", async () => {
  await main(["1000", "--", process.execPath, "-e", "process.exit(0)"]);
});

it("rejects a long-validation deadline above five minutes", async () => {
  await assert.rejects(
    main(["300001", "--", process.execPath, "-e", "process.exit(0)"]),
    /within 300000ms/i,
  );
});
