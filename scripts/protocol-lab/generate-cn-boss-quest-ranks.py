"""Extract difficulty IDs from the CN Android boss_battle_quest orderedmap.

Usage: --source <master/quest/boss_battle_quest.orderedmap> --output <json>
Field 107 is battle_quest_rank in Android BossBattleQuestValues (1.8.1).
Difficulty IDs are not ordered by quest suffix (e.g. White Tiger 3 is 5,
while White Tiger 4 is 4), so mission matching must use this master.
"""
import argparse
import json
from pathlib import Path

from cn_gacha_banner_assets import decode_ordered_map


def extract(rows):
    result = {}
    for world_id, chapters in rows.items():
        for chapter_id, quests in chapters.items():
            for quest_id, row in quests.items():
                multiplied_id = int(world_id) * 1_000_000 + int(chapter_id) * 1_000 + int(quest_id)
                if int(row[0]) != multiplied_id:
                    raise ValueError(f"Mismatched boss quest ID {multiplied_id}")
                result[f"2:{multiplied_id}"] = int(row[107])
    if len(result) != 232:
        raise ValueError(f"Expected 232 CN boss quests, got {len(result)}")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    ranks = extract(decode_ordered_map(args.source.read_bytes()))
    args.output.write_text(json.dumps(ranks, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")
    print(json.dumps({"boss_quests": len(ranks)}))
