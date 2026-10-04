import assert from "node:assert/strict"
import fs from "node:fs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { patchTutorialGachaFallback } from "./patch-cn-tutorial-gacha.mjs"

const master = decodeOrderedMap(fs.readFileSync(new URL(
    "../../core/personal-service/assets/cn-gacha-repair-masters/gacha.orderedmap", import.meta.url)))
// Mirror Android GachaRepository.getGachaIdCanbeTutorialAtTime: no end-time filter.
function candidate(rows, time) {
    return Object.entries(rows)
        .filter(([, row]) => row[38] === "true" && row[43] !== "true"
            && row[46] !== "true" && row[29] <= time)
        .sort((a, b) => b[1][30].localeCompare(a[1][30]))[0]?.[0]
}
const before = structuredClone(master)
before[1].splice(38, 5, "false", "", "", "", "")
const fixed = patchTutorialGachaFallback(before)
assert.equal(candidate(before, "2019-12-02 12:00:00"), undefined, "reproduce native C3043")
assert.equal(candidate(fixed, "2019-12-02 12:00:00"), "1")
assert.equal(candidate(master, "2019-12-02 12:00:00"), "1", "embedded Android master must contain fix")
assert.equal(candidate(fixed, "2025-06-01 12:00:00"), candidate(before, "2025-06-01 12:00:00"),
    "later timeline must retain the original tutorial selection")
assert.notEqual(candidate(fixed, "2025-06-01 12:00:00"), "1")
for (const [id, row] of Object.entries(before)) {
    if (id !== "1") assert.deepEqual(fixed[id], row)
    else row.forEach((value, index) => {
        if (index < 38 || index > 42) assert.equal(fixed[id][index], value)
    })
}
assert.equal(before[1][38], "false", "must not mutate the source")
assert.deepEqual(fixed[1].slice(38, 43), ["true", "tutorial_rarity", "normal_guarantee", "150", "72"])
assert.deepEqual(patchTutorialGachaFallback(fixed), fixed, "idempotent repair")
assert.throws(() => patchTutorialGachaFallback({}), /invalid/)
console.log("Tutorial gacha: C3043 reproduction, embedded early fallback, later selection, unrelated fields and idempotency passed")
