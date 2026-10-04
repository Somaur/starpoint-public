// Enable quantity selection for mana goods without changing their daily stock.
import fs from "node:fs"
import path from "node:path"
import crypto from "node:crypto"
import assert from "node:assert/strict"
import { fileURLToPath } from "node:url"
import { encodeOrderedMap } from "./encode-cn-orderedmap.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"

export function patchManaShop(master) {
    const rows = structuredClone(master)
    const limits = {}
    for (const [id, row] of Object.entries(rows)) {
        // TreasureShop: price_kind=7, buy_max_count=21, daily_stock=23.
        if (row[7] !== "1" || row[21] !== "1") continue
        const dailyStock = Number(row[23])
        if (!Number.isSafeInteger(dailyStock) || dailyStock <= 1) continue
        const maximum = Math.min(dailyStock, 100)
        row[21] = String(maximum)
        limits[id] = maximum
    }
    return { rows, limits }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    const [input, output] = process.argv.slice(2)
    if (!input || !output) throw new Error("usage: build-cn-mana-shop-master.mjs source.json output-directory")
    const source = JSON.parse(fs.readFileSync(input, "utf8"))
    const { rows, limits } = patchManaShop(source.rows)
    const bytes = encodeOrderedMap(rows)
    assert.deepEqual(decodeOrderedMap(bytes), rows)
    fs.mkdirSync(output, { recursive: true })
    fs.writeFileSync(path.join(output, "treasure_shop.orderedmap"), bytes)
    const manifest = {
        entryPath: source.entryPath, sourceSha256: source.sha256,
        sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
        rows: Object.keys(rows).length, limits,
    }
    fs.writeFileSync(path.join(output, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n")
    console.log(JSON.stringify({ rows: manifest.rows, changed: Object.keys(limits).length }))
}
