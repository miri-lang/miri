
# Miri VS Code Extension    

This extension provides syntax highlighting and language support for the Miri programming language.

**To install:**
1. Navigate to the extension folder: `cd miri-extension`
2. Install dependencies: `npm install`
3. Package the extension: `npx vsce package`
4. Install the generated `.vsix` file in VS Code:
   - Open VS Code
   - Go to Extensions (`Cmd+Shift+X` or `Ctrl+Shift+X`)
   - Click the `...` menu -> **Install from VSIX...**
   - Select the generated file (e.g., `miri-language-0.1.0.vsix`)
