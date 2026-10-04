// Complete the client feature table before exposing canonical and temporary pools.
// Missing promotional images use that pool's available list banner; odds stay intact.
export function patchGachaFeatures(originalGacha, patchedGacha, originalFeatures, policy, missingImages) {
    const result = structuredClone(originalFeatures)
    const repairs = { inherited: {}, imageFallbacks: [] }
    for (const [id, row] of Object.entries(originalGacha)) {
        if (!result[id]) {
            const source = Object.keys(originalFeatures).find(candidate =>
                originalGacha[candidate]?.[3] === row[3])
            if (!source) throw new Error(`no feature source for gacha ${id}`)
            result[id] = structuredClone(originalFeatures[source])
            repairs.inherited[id] = source
        }
    }
    for (const [id, rows] of Object.entries(result)) {
        for (const [index, row] of Object.entries(rows)) {
            if (row[0] === "1" && missingImages.has(row[1])) {
                if (!patchedGacha[id]?.[3]) throw new Error(`missing fallback banner for ${id}`)
                repairs.imageFallbacks.push({ id, index, original: row[1], fallback: patchedGacha[id][3] })
                row[1] = patchedGacha[id][3]
            }
        }
    }
    for (const aliases of [policy.normalizedCoverageAliases, policy.temporaryAliases]) {
        for (const [id, canonical] of Object.entries(aliases)) {
            if (!result[canonical]) throw new Error(`missing canonical feature ${canonical}`)
            result[id] = structuredClone(result[canonical])
        }
    }
    for (const id of Object.keys(patchedGacha)) {
        if (!result[id] || !Object.keys(result[id]).length) throw new Error(`empty feature table ${id}`)
    }
    return { features: result, repairs }
}
