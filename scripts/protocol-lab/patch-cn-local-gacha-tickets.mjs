// Allow the local launch pool to consume the generic tickets granted by the service.
export function patchLocalGachaTickets(master, region, policy) {
    const result = structuredClone(master)
    const enabled = new Set(policy.characterWildcardPools.map(String))
    for (const [id, row] of Object.entries(result)) {
        const canonical = String(region.temporaryAliases[id] ?? region.normalizedCoverageAliases[id] ?? id)
        if (!enabled.has(canonical)) continue
        if (row[13] !== "0" || row[4] !== "0") throw new Error(`not a normal character pool: ${id}`)
        row[20] = "true"
    }
    return result
}
