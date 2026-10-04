// Validate the Android overlay without requiring the legacy extraction bundle.
import assert from "node:assert/strict"
import fs from "node:fs"
import { applyAndroidBattleContract } from "./apply-cn-battle-contract.mjs"

const read = relative => JSON.parse(fs.readFileSync(new URL(relative, import.meta.url), "utf8"))
const fixture = read("../../core/personal-service/assets/cn-single-battle.json")
const contract = read("../../core/personal-service/assets/cn-battle-contract.json")
const rewards = read("../../assets/clear_reward.json")
assert.equal(Object.keys(contract.quests).length, 5250)
for (const [key, row] of Object.entries(contract.quests)) {
    const actual = fixture.quests[key]
    for (const field of ["stamina_cost", "rank_point_reward", "character_exp_reward", "mana_reward", "pool_exp_reward"]) {
        if (row[field] !== undefined) assert.equal(actual[field], row[field], `${key}.${field}`)
    }
    for (const field of ["clear_reward", "s_plus_reward"]) {
        const id = row[`${field}_id`]
        assert.equal(actual[`${field}_id`] ?? null, id, `${key}.${field}_id`)
        if (id === null) assert.equal(actual[field], undefined, `${key}.${field}`)
        else {
            const expected = Object.fromEntries(["type", "id", "count", "rarity"].filter(k => rewards[id][k] !== undefined).map(k => [k, rewards[id][k]]))
            assert.deepEqual(actual[field], expected, `${key}.${field}`)
        }
    }
}
// Reapplying must not turn absent first-clear rewards into the legacy 15-star default.
const reapplied = JSON.parse(JSON.stringify(applyAndroidBattleContract(structuredClone(fixture), rewards)))
assert.deepEqual(reapplied, fixture)
assert.equal(fixture.quests["27:1155"].stamina_cost, 10)
assert.equal(fixture.quests["27:1155"].rank_point_reward, 10)
assert.equal(fixture.quests["27:1155"].clear_reward, undefined)
assert.equal(fixture.quests["25:1001"].rank_point_reward, 294)
assert.equal(fixture.quests["26:1001"].clear_reward_id, 34)
assert.deepEqual(fixture.quests["26:1001"].clear_reward, {type:3,count:30})
console.log(JSON.stringify({quests:5250,firstClearAndSS:true,economy:true,idempotent:true}))
