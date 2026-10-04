// Android SoloTimeAttackClearTimeRewardValues: threshold seconds and six general rewards.
import fs from "node:fs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"

const assets = new URL("../../core/personal-service/assets/", import.meta.url)
const read = name => decodeOrderedMap(fs.readFileSync(new URL(`cn-activity-masters/${name}.orderedmap`, assets)))
const quests = read("solo_time_attack_event_quest")
const kinds = { 0: 0, 1: 1, 2: 3, 3: 4, 4: 5, 6: 2 }
const stages = Object.entries(read("solo_time_attack_clear_time_reward")).map(([id, row]) => {
    const quest = quests[row[1]]?.[row[2]]
    if (!quest) throw new Error(`Unknown solo quest ${row[1]}:${row[2]}`)
    const stage = { id: Number(id), event_id: Number(row[1]), quest_id: Number(quest[0]), time_ms: Number(row[4]) * 1000, rewards: [], degrees: [] }
    for (let base = 6; base < 24; base += 3) {
        if (row[base] === "(None)" || row[base] === "") continue
        const kind = Number(row[base])
        if (kind === 7) { stage.degrees.push(Number(row[base + 1])); continue }
        if (!(kind in kinds)) throw new Error(`Unsupported general reward ${kind}`)
        stage.rewards.push({ type: kinds[kind], id: Number(row[base + 1]) || null, count: Number(row[base + 2]), rarity: null })
    }
    return stage
}).sort((a, b) => a.id - b.id)
const output = new URL("cn-solo-time-rewards.json", assets)
const serialized = `${JSON.stringify(stages)}\n`
if (process.argv.includes("--check")) {
    if (fs.readFileSync(output, "utf8") !== serialized) throw new Error("Solo reward fixture differs")
} else fs.writeFileSync(output, serialized)
console.log(JSON.stringify({ stages: stages.length, quests: new Set(stages.map(s => s.quest_id)).size }))
