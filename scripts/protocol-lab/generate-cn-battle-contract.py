"""Extract CN Android 1.8.1 entry costs and settlement rewards.

--source-root contains the 21 original quest .orderedmap files, named after
SOURCES. Field offsets are from Android's generated *QuestValues classes;
0 is Once (quest/unlock payment), 1 is Always (on-clear payment). The seven legacy-only
practice quests have no Android rows and retain their original fixture data.
"""
import argparse
import hashlib
import json
from pathlib import Path
from cn_gacha_banner_assets import decode_ordered_map

# Offsets verified against the installed Android generated QuestValues classes.
ECONOMY_COLUMNS = json.loads((Path(__file__).with_name('cn-battle-economy-columns.json')).read_text(encoding='utf-8'))

# category, SS reward column (None = no SS reward), item mode column, row count
SOURCES = {'main_quest': [1, 71, 55, 419],
 'boss_battle_quest': [2, 71, 55, 232],
 'character_quest': [3, 73, 57, 1318],
 'ex_quest': [4, 71, 55, 221],
 'daily_week_event_quest': [6, None, 50, 114],
 'advent_event_quest': [7, 77, 61, 459],
 'story_event_single_quest': [10, 73, 57, 348],
 'ranking_event_single_quest': [11, None, 52, 7],
 'challenge_dungeon_event_quest': [13, 72, 56, 46],
 'daily_exp_mana_event_quest': [14, None, 51, 6],
 'practice_quest': [15, 72, 56, 91],
 'world_story_event_quest': [18, 72, 56, 913],
 'world_story_event_boss_battle_quest': [19, 71, 55, 96],
 'tower_dungeon_event_quest': [20, None, 54, 480],
 'expert_single_event_quest': [21, 74, 58, 28],
 'carnival_event_quest': [22, None, 53, 171],
 'raid_event_quest': [23, None, 54, 50],
 'rush_event_quest': [24, None, 53, 110],
 'solo_time_attack_event_quest': [25, None, 56, 6],
 'hard_multi_event_quest': [26, 72, 56, 12],
 'score_attack_event_quest': [27, None, 57, 123]}

def leaves(value, keys=()):
    if isinstance(value, list):
        yield keys, value
    else:
        for key, child in value.items():
            yield from leaves(child, keys + (int(key),))

def integer(value):
    return None if value in (None, '', '(None)') else int(value)

def extract(source_root):
    quests, sources = {}, {}
    for name, (category, ss, mode_index, expected) in SOURCES.items():
        raw = (source_root / (name + '.orderedmap')).read_bytes()
        count = 0
        for keys, row in leaves(decode_ordered_map(raw)):
            quest_id = 0
            for key in keys:
                quest_id = quest_id * 1000 + key
            mode = integer(row[mode_index])
            if mode not in (None, 0, 1):
                raise ValueError(f'Unknown item cost mode {name}:{quest_id}: {mode}')
            items = []
            if mode is not None:
                ids = [int(x) for x in row[mode_index+1].split(',') if x]
                counts = [int(x) for x in row[mode_index+2].split(',') if x]
                if len(ids) != len(counts) or any(x <= 0 for x in ids + counts):
                    raise ValueError(f'Invalid item cost {name}:{quest_id}')
                items = [dict(id=i, count=n) for i, n in zip(ids, counts)]
            quests[f'{category}:{quest_id}'] = dict(s_plus_reward_id=integer(row[ss]) if ss is not None else None, completion_items=items if mode == 1 else [], unlock_items=items if mode == 0 else [])
            for field, index in ECONOMY_COLUMNS[name].items():
                value = integer(row[index])
                quests[f'{category}:{quest_id}'][field] = value if field == 'clear_reward_id' else (value or 0)
            count += 1
        if count != expected:
            raise ValueError(f'{name}: expected {expected} rows, got {count}')
        sources[name] = dict(sha256=hashlib.sha256(raw).hexdigest(), count=count, ss_column=ss, cost_mode_column=mode_index, economy_columns=ECONOMY_COLUMNS[name])
    return dict(client='CN Android 1.8.1', sources=sources, quests=quests)

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = extract(args.source_root)
    args.output.write_text(json.dumps(result, sort_keys=True, separators=(',', ':')) + '\n', encoding='utf-8')
    print(json.dumps(dict(quests=len(result['quests']), sources=len(result['sources']))))
