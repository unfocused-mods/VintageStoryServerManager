import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const child=spawn(process.execPath,[path.join(root,'node_modules/@tauri-apps/cli/tauri.js'),...process.argv.slice(2)],{cwd:path.join(root,'apps/desktop'),stdio:'inherit'});
child.on('exit',code=>process.exit(code??1));
