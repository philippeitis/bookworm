# This script exists because the code below
# Command::new("explorer.exe").arg(format!("/select,\"{}\"", variant.display())).arg("> command.txt").spawn().expect("???");
# escapes the necessary quotes, breaking the explorer call.
import sys
import subprocess
import os

if __name__ == '__main__':
    FILEBROWSER_PATH = os.path.join(os.getenv('WINDIR'), 'explorer.exe')
    subprocess.Popen(f'{FILEBROWSER_PATH} /select,"{sys.argv[1]}"')
