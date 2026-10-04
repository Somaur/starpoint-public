"""Generate CN daily challenge data from decoded Android orderedmap rows."""
import argparse
import datetime
import json
from pathlib import Path

def timestamp(raw):
    if raw in ('', '(None)'):
        return None
    return int(datetime.datetime.fromisoformat(raw).replace(
        tzinfo=datetime.timezone(datetime.timedelta(hours=8))).timestamp())

def generate(source):
    return {
        'points': {key: {'max': int(row[1]), 'recovers': row[3] == 'true'}
                   for key, row in source['points'].items()},
        'campaigns': {key: {'point_id': int(row[1]), 'additional': int(row[2]),
                            'start': timestamp(row[3]), 'end': timestamp(row[4])}
                      for key, row in source['campaigns'].items()},
    }

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = generate(json.loads(args.source.read_text(encoding='utf-8')))
    args.output.write_text(json.dumps(result, separators=(',', ':'))+'\n', encoding='utf-8')
    print({key: len(rows) for key, rows in result.items()})
