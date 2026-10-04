// Android excludes comeback/stars pools from its tutorial candidate search.
// Pool 1 covers the early local timeline; CN's original tutorial pools begin in 2021.
export function patchTutorialGachaFallback(master) {
    const fallback = master["1"]
    const template = master["63"]
    if (!Array.isArray(fallback) || fallback.length <= 46
        || !Array.isArray(template) || template.length <= 46
        || fallback[43] === "true" || fallback[46] === "true"
        || fallback[13] !== template[13] || template[38] !== "true"
        || template.slice(39, 43).some(value => !value)) {
        throw new Error("invalid early tutorial gacha fallback/template")
    }
    const row = [...fallback]
    // Reuse validated client rarity/movie/cost/reason fields, leaving normal draws unchanged.
    row.splice(38, 5, ...template.slice(38, 43))
    return { ...master, 1: row }
}
