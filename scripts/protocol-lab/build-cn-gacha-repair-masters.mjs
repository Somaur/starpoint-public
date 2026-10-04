// Input is an audit of the actual Android release masters and image availability.
import fs from "node:fs"
import path from "node:path"
import crypto from "node:crypto"
import assert from "node:assert/strict"
import { encodeOrderedMap } from "./encode-cn-orderedmap.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { applyCnGachaRegionPolicy, applyCnGachaCampaignPolicy, applyCnFeatureBannerRegionPolicy } from "./patch-cn-gacha-master.mjs"
import { patchGachaFeatures } from "./patch-cn-gacha-features.mjs"
import { patchTutorialGachaFallback } from "./patch-cn-tutorial-gacha.mjs"
import { patchLocalGachaTickets } from "./patch-cn-local-gacha-tickets.mjs"

const [input, output] = process.argv.slice(2)
if (!input || !output) throw new Error("usage: build-cn-gacha-repair-masters.mjs audit.json output-directory")
const source = JSON.parse(fs.readFileSync(input, "utf8"))
const policy = JSON.parse(fs.readFileSync(new URL("../../assets/gacha-region-policy.json", import.meta.url)))
const campaigns = JSON.parse(fs.readFileSync(new URL("../../assets/gacha_campaign.json", import.meta.url)))
const original = source.masters.gacha.rows
const tickets = JSON.parse(fs.readFileSync(new URL("../../assets/gacha-local-ticket-policy.json", import.meta.url)))
const gacha = patchLocalGachaTickets(patchTutorialGachaFallback(applyCnGachaRegionPolicy(original, policy)), policy, tickets)
const { features, repairs } = patchGachaFeatures(original, gacha, source.masters.gacha_feature_content.rows,
    policy, new Set(source.missingFeatureImages))
// The launch prince movie contains a damaged title atlas. Keep its preview card,
// and show the matching portrait already present in the Android asset bundle.
for (const rows of Object.values(features)) {
    for (const row of Object.values(rows)) {
        if (row[0] === "0" && row[2] === "gacha/feature_movie/release_gacha/prince_zero/feature") {
            row[0] = "1"
            row[1] = "character/prince_zero/ui/full_shot_1440_1920_0"
            row[2] = ""
        }
    }
}
assert.equal(source.masters.feature_banner.sha256, policy.featureLinkSources.feature_banner)
const homeBanners = applyCnFeatureBannerRegionPolicy(source.masters.feature_banner.rows, "feature_banner", policy)
// The original launch home banner is absent. Use the pool's verified list banner.
assert.equal(homeBanners["1"][28], "dynamic/home_banner/gacha/release_gacha_home_banner_01")
homeBanners["1"][28] = gacha["1"][3]
const values = {
    gacha,
    gacha_feature_content: features,
    gacha_campaign: applyCnGachaCampaignPolicy(source.masters.gacha_campaign.rows, policy, campaigns),
    feature_banner: homeBanners,
}
fs.mkdirSync(output, {recursive: true})
const manifest = { formatVersion: 1, sourceBundleSha256: source.sourceBundleSha256, masters: [], repairs }
for (const [name, rows] of Object.entries(values)) {
    const bytes = encodeOrderedMap(rows)
    assert.deepEqual(decodeOrderedMap(bytes), rows)
    fs.writeFileSync(path.join(output, `${name}.orderedmap`), bytes)
    manifest.masters.push({name, entryPath: source.masters[name].entryPath, sourceSha256: source.masters[name].sha256,
        sha256: crypto.createHash("sha256").update(bytes).digest("hex"), rows: Object.keys(rows).length})
}
fs.writeFileSync(path.join(output, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n")
console.log(JSON.stringify({masters:manifest.masters, inheritedFeatures:Object.keys(repairs.inherited).length,
    fallbackImages:repairs.imageFallbacks.length}, null, 2))
