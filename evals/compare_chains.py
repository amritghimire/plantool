#!/usr/bin/env python3
"""Compare exported eval evidence; never calls models or changes provider policy."""
import argparse,csv,json
from pathlib import Path

def compare(tasks, rows):
    expected={task['id']:set(task['expected_tests']) for task in tasks}
    result={}
    seen=set()
    for row in rows:
        key=(row['chain'],row['task'])
        if key in seen: raise ValueError('Duplicate chain/task sample')
        seen.add(key)
        if row['task'] not in expected: raise ValueError('Unknown eval task')
        checks=row.get('checks',[])
        passed={c['command'] for c in checks if c.get('status')=='pass' and c.get('evidence','').startswith('artifact://')}
        valid=row.get('merged',False) and expected[row['task']]<=passed
        summary=result.setdefault(row['chain'],dict(samples=0,completed=0,retries=0,owner_interventions=0))
        summary['samples']+=1;summary['completed']+=int(valid)
        summary['retries']+=row.get('retries',0);summary['owner_interventions']+=row.get('owner_interventions',0)
    return [{'chain':chain,**data,'completion_rate':round(data['completed']/data['samples'],4)} for chain,data in sorted(result.items())]

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--results',type=Path);parser.add_argument('--output',type=Path);parser.add_argument('--self-test',action='store_true');args=parser.parse_args()
    tasks=json.loads(Path(__file__).with_name('tasks.json').read_text())
    if args.self_test:
        good={'chain':'fake-a','task':'E1','merged':True,'retries':1,'owner_interventions':2,'checks':[{'command':tasks[0]['expected_tests'][0],'status':'pass','evidence':'artifact://'+'a'*64}]}
        bad={**good,'chain':'fake-b','checks':[]}
        report=compare(tasks,[good,bad]);assert report[0]['completed']==1 and report[1]['completed']==0
        return
    if not args.results:parser.error('--results is required')
    report=compare(tasks,json.loads(args.results.read_text()))
    if args.output:
        with args.output.open('w',newline='') as stream:
            writer=csv.DictWriter(stream,fieldnames=['chain','samples','completed','retries','owner_interventions','completion_rate']);writer.writeheader();writer.writerows(report)
    else: print(json.dumps(report,indent=2))
if __name__=='__main__':main()
