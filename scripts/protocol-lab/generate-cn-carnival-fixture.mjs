// Decode the Android CarnivalEventQuest/TotalScoreReward contract.
import fs from "node:fs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"

const assets = new URL("../../core/personal-service/assets/", import.meta.url)
const read = (name) => decodeOrderedMap(fs.readFileSync(new URL(`cn-activity-masters/${name}.orderedmap`, assets)))
const scores = {}
for (const [event, quests] of Object.entries(read("carnival_event_quest"))) {
    for (const row of Object.values(quests)) {
        scores[row[0]] = {
            event_id: Number(event), folder_id: Number(row[1]), legacy_folder_id: Number(row[2]),
            difficulty_score: Number(row[104]),
            // battle_time_limit is a count of frames at 60 FPS.
            time_limit_ms: Number(row[100]) * 1000 / 60,
        }
    }
}
const rewardKinds = { 0: 0, 1: 1, 2: 3, 3: 4, 4: 5, 6: 2 }
const rewards = Object.entries(read("carnival_event_total_score_reward")).map(([id, row]) => {
    const value = { id: Number(id), event_id: Number(row[0]), score: Number(row[2]), rewards: [], degrees: [] }
    for (let base = 4; base < row.length; base += 3) {
        if (row[base] === "(None)" || row[base] === "") continue
        const kind = Number(row[base])
        if (kind === 7) { value.degrees.push(Number(row[base + 1])); continue }
        if (!(kind in rewardKinds)) throw new Error(`Unsupported general reward ${kind}`)
        value.rewards.push({ type: rewardKinds[kind], id: Number(row[base + 1]) || null, count: Number(row[base + 2]), rarity: null })
    }
    return value
})
for (const [name, value] of [["cn-carnival-quest-scores.json", scores], ["cn-carnival-score-rewards.json", rewards]]) {
    const output = new URL(name, assets)
    const serialized = `${JSON.stringify(value)}\n`
    if (process.argv.includes("--check")) {
        if (fs.readFileSync(output, "utf8") !== serialized) throw new Error(`Fixture differs: ${name}`)
    } else fs.writeFileSync(output, serialized)
}
console.log(JSON.stringify({ quests: Object.keys(scores).length, reward_stages: rewards.length }))
