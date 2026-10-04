// Android master fields are authoritative for entry costs and settlement rewards.
import fs from "node:fs"
import { fileURLToPath } from "node:url"

export function applyAndroidBattleContract(fixture, clearRewards) {
    const contract = JSON.parse(fs.readFileSync(new URL("../../core/personal-service/assets/cn-battle-contract.json", import.meta.url), "utf8"))
    for (const [key, value] of Object.entries(contract.quests)) {
        const quest = fixture.quests[key]
        if (!quest) throw new Error(`Android contract references missing quest ${key}`)
        const reward = value.s_plus_reward_id === null ? undefined : clearRewards[String(value.s_plus_reward_id)]
        if (value.s_plus_reward_id !== null && !reward) throw new Error(`Missing Android SS reward ${key}`)
        quest.s_plus_reward_id = value.s_plus_reward_id ?? undefined
        quest.s_plus_reward = reward ? Object.fromEntries(["type", "id", "count", "rarity"].filter(field => reward[field] !== undefined).map(field => [field, reward[field]])) : undefined
        // Always items are consumed on clear. Once items belong to quest/unlock.
        delete quest.entry_item_id
        quest.entry_item_count = 0
        quest.completion_items = value.completion_items
        delete quest.completion_items_once
        for (const field of ["stamina_cost", "rank_point_reward", "character_exp_reward", "mana_reward", "pool_exp_reward"]) {
            if (value[field] !== undefined) quest[field] = value[field]
        }
        const clearReward = value.clear_reward_id === null ? undefined : clearRewards[String(value.clear_reward_id)]
        if (value.clear_reward_id !== null && !clearReward) throw new Error(`Missing Android first-clear reward ${key}`)
        quest.clear_reward_id = value.clear_reward_id ?? undefined
        quest.clear_reward = clearReward ? Object.fromEntries(["type", "id", "count", "rarity"].filter(field => clearReward[field] !== undefined).map(field => [field, clearReward[field]])) : undefined
    }
    fixture.source.included_clear_reward_count = new Set(Object.values(fixture.quests).flatMap(q => [q.clear_reward_id, q.s_plus_reward_id]).filter(id => id !== undefined && id !== null)).size
    return fixture
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
    const [fixturePath, rewardsPath] = process.argv.slice(2)
    if (!fixturePath || !rewardsPath) throw new Error("Usage: apply-cn-battle-contract.mjs <fixture.json> <clear_reward.json>")
    const fixture = applyAndroidBattleContract(JSON.parse(fs.readFileSync(fixturePath, "utf8")), JSON.parse(fs.readFileSync(rewardsPath, "utf8")))
    const sort = value => Array.isArray(value) ? value.map(sort) : value && typeof value === "object" ? Object.fromEntries(Object.keys(value).sort((a, b) => a.localeCompare(b, "en")).map(key => [key, sort(value[key])])) : value
    fs.writeFileSync(fixturePath, JSON.stringify(sort(fixture)) + "\n")
    console.log(JSON.stringify({ quests: Object.keys(fixture.quests).length }))
}
