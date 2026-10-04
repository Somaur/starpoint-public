// Android score milestones may contain several rewards per level, all paid once.
import fs from "node:fs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
const assets = new URL("../../core/personal-service/assets/", import.meta.url)
const read = name => decodeOrderedMap(fs.readFileSync(new URL(`cn-activity-masters/${name}.orderedmap`, assets)))
const quests = read("score_attack_event_quest")
const stages = Object.entries(read("score_attack_border_reward")).map(([id, row]) => {
    const quest = quests[row[1]]?.[row[2]]
    if (!quest) throw new Error(`Unknown score quest ${row[1]}:${row[2]}`)
    const stage = { id: Number(id), event_id: Number(row[1]), quest_id: Number(quest[0]), score: Number(row[4]), rewards: [] }
    for (let base = 6; base < 24; base += 3) {
        if (row[base] === "(None)" || row[base] === "") continue
        if (row[base] !== "0") throw new Error(`Unexpected score reward kind ${row[base]}`)
        stage.rewards.push({ type: 0, id: Number(row[base + 1]), count: Number(row[base + 2]), rarity: null })
    }
    return stage
}).sort((a, b) => a.id - b.id)
const output = new URL("cn-score-milestones.json", assets)
const serialized = `${JSON.stringify(stages)}\n`
if (process.argv.includes("--check")) {
    if (fs.readFileSync(output, "utf8") !== serialized) throw new Error("Score milestones differ")
} else fs.writeFileSync(output, serialized)
console.log(JSON.stringify({ stages: stages.length, quests: new Set(stages.map(s => s.quest_id)).size }))
// Rank fields are scores in columns 52..55, never elapsed-time milliseconds.
const ranks = Object.fromEntries(Object.values(quests).flatMap(event =>
    Object.values(event).map(row => [row[0], row.slice(52, 56).map(Number)])))
const rankOutput = new URL("cn-score-quest-ranks.json", assets)
const rankSerialized = `${JSON.stringify(ranks)}\n`
if (process.argv.includes("--check")) {
    if (fs.readFileSync(rankOutput, "utf8") !== rankSerialized) throw new Error("Score ranks differ")
} else fs.writeFileSync(rankOutput, rankSerialized)
