# Exceção para plugins

O Code Terminal é licenciado sob a GNU General Public License, versão 3 ou
posterior (veja [LICENSE](LICENSE)). Esta exceção, concedida pela Código
Central, detentora dos direitos autorais, garante que quem escreve plugins
pode escolher a licença que quiser, inclusive uma licença fechada.

**Em resumo:** um plugin WASM que conversa com o Code Terminal só pela Plugin
API pública não é obra derivada do Code Terminal. A GPL não se aplica a ele.

O texto em inglês abaixo é o que vale juridicamente.

---

## Code Terminal Plugin Exception

Additional permission under GNU GPL version 3 section 7

As a special exception, the copyright holder of Code Terminal (Código
Central) gives you permission to create, use and distribute Plugins under
terms of your choice, regardless of the license terms of Code Terminal,
provided that each Plugin:

1. is compiled to WebAssembly and loaded by Code Terminal at run time; and
2. interacts with Code Terminal exclusively through the Public Plugin API.

"Public Plugin API" means the host functions, data formats and manifest
fields that Code Terminal exposes to WebAssembly modules and documents in
`docs/PLUGIN_API.md`, including the interfaces generated from them.

"Plugin" means a work that meets the two conditions above and that contains
no part of Code Terminal other than material from the Public Plugin API that
is necessary to call it (such as function signatures, constants and data
structure definitions).

A Plugin that meets these conditions is not considered a work based on Code
Terminal for the purposes of the GNU GPL. This exception does not apply to
modifications of Code Terminal itself, which remain covered by the GNU GPL
version 3 or later.

If you modify Code Terminal, you may extend this exception to your version,
but you are not obligated to do so. If you do not wish to do so, delete this
exception statement from your version.
