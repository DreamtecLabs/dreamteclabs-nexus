from pathlib import Path

path = Path("ui/src/nexus/inventory.rs")
text = path.read_text()
old = '''        Callback::from(move |event: MouseEvent| {
            let Some(target) = event
'''
new = '''        Callback::from(move |event: MouseEvent| {
            if event.button() != 0 {
                return;
            }

            let Some(target) = event
'''
if old not in text:
    raise SystemExit("table interaction callback not found")
path.write_text(text.replace(old, new, 1))
