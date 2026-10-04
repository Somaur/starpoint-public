import assert from "node:assert/strict"
import { patchGachaFeatures } from "./patch-cn-gacha-features.mjs"

const row = ["pool", "Title", "100", "dynamic/banner"]
const original = { 1: row, 61: [...row] }
const patched = { ...original, 9000001: [...row] }
const features = { 1: { 1: ["1", "dynamic/missing", "", "", "", "", "(None)", "", ""] } }
const policy = { normalizedCoverageAliases: {61: 1}, temporaryAliases: {9000001: 1} }
const fixed = patchGachaFeatures(original, patched, features, policy, new Set(["dynamic/missing"]))
for (const id of Object.keys(patched)) assert.equal(fixed.features[id][1][1], "dynamic/banner")
assert.equal(features[1][1][1], "dynamic/missing", "source must remain unchanged")
fixed.features[61][1][1] = "modified"
assert.equal(fixed.features[1][1][1], "dynamic/banner", "aliases must not share mutable rows")
assert.throws(() => patchGachaFeatures({2: ["x", "x", "1", "different"]}, {2: row}, features, policy, new Set()), /no feature source/)
console.log("Gacha feature inheritance, missing-image fallback, alias isolation and invalid-input tests passed")
