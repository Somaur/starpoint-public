import assert from 'node:assert/strict'
import { patchLocalGachaTickets } from './patch-cn-local-gacha-tickets.mjs'
const row = Array(25).fill('')
row[4] = '0'; row[13] = '0'; row[20] = 'false'
const master = {1:[...row], 2:[...row], 61:[...row], 9001:[...row]}
const region = {normalizedCoverageAliases:{61:1}, temporaryAliases:{9001:1}}
const fixed = patchLocalGachaTickets(master,region,{characterWildcardPools:[1]})
for (const id of [1,61,9001]) assert.equal(fixed[id][20],'true')
assert.equal(fixed[2][20],'false')
assert.equal(master[1][20],'false')
master[1][13] = '1'
assert.throws(()=>patchLocalGachaTickets(master,region,{characterWildcardPools:[1]}),/not a normal character pool/)
console.log('Local ticket policy: launch pool, aliases, unrelated pools, source immutability and invalid pool type passed')
