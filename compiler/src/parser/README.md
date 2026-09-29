# Parser da SpectraLang

O parser é um parser descendente recursivo que converte tokens em AST. Aceitar
uma forma sintática não prova que a análise semântica, o lowering ou os
backends consigam validá-la e executá-la; consulte
[`docs/frontend/frontend-coverage-audit.md`](../../../docs/frontend/frontend-coverage-audit.md)
e [`docs/semantic/semantic-coverage-audit.md`](../../../docs/semantic/semantic-coverage-audit.md)
para essas fronteiras.

## Organização

- `mod.rs`: estado do parser, navegação de tokens, diagnóstico, recuperação e
  limite de recursão.
- `workspace.rs`: parsing de conjuntos de módulos/projetos.
- `module.rs`: cabeçalhos `module` e imports.
- `item.rs` e módulos `item_*`: despacho de itens, declarações, assinaturas,
  traits, impls e aliases.
- `statement.rs`: bindings `let`, atribuições, controle de fluxo e statements.
- `expression.rs`, `expression_primary.rs` e `expression_precedence.rs`:
  expressões, chamadas, operadores e precedência.
- `expression_patterns.rs`: padrões, alternativas OR e padrões de enum.
- `type_annotation.rs`: tipos simples, compostos, qualificados e genéricos.

## Superfície sintática

O parser reconhece módulos/imports, funções, records, enums, traits e impls,
tipos genéricos, `const`/`static`, closures e os controles de fluxo documentados
(`if`, `if let`, `while`, `while let`, `for ... in`, `loop`, `do ... while`,
`switch` e `match`). As expressões incluem chamadas e métodos, acesso/indexação,
casts, blocos, f-strings, async/`await` e propagação `?`.

Bindings `let` aceitam `mut` como marcador redundante. Padrões de `match`
incluem wildcard, binding, literal, tupla, variantes de enum, forma struct e
OR-pattern. Braços aceitam guarda com a forma `when padrão if expressão then
corpo`; o parser armazena a guarda na AST.

O código-fonte termina statements por linha/estrutura de blocos; ponto e vírgula
não é o terminador canônico. Não há gates sintáticos experimentais ativos;
`--enable-experimental` é uma opção de compatibilidade sem efeito.

## Validação semântica depois do parsing

- A guarda é analisada no escopo das bindings do padrão e precisa ser `bool`.
  Um braço guardado não oferece cobertura incondicional para exaustividade.
- `?` aceita apenas `Option<T>` e `Result<T, E>`, produz o payload de sucesso e
  exige que o tipo de retorno da função aceite a propagação de `None`/`Err`.
- `mut` não cria uma categoria separada de bindings imutáveis; variáveis locais
  podem ser reatribuídas por padrão.

Diagnósticos e fixtures correspondentes ficam nas validações frontend/semantic;
esta nota descreve o contrato, não certifica que os gates foram executados no
checkout atual.

## Construção e recuperação de erros

`Parser::parse()` percorre o módulo e delega cada construção ao módulo
responsável. Erros carregam spans localizados. A rotina de sincronização tenta
continuar após uma falha, e um limite de profundidade impede recursão excessiva
de consumir a pilha.

Exemplo de uso com a sintaxe atual:

```rust
use spectra_compiler::{Lexer, Parser};

let source = r#"
module example

public func main() returns int {
    let mut total = 40
    total = total + 2
    return total
}
"#;

let tokens = Lexer::new(source).tokenize().unwrap();
let module = Parser::new(tokens).parse().unwrap();
```

## Sintaxe reservada ou adiada

`class` é reservado e rejeitado com diagnóstico `P007`. Unicode identifiers,
lifetime syntax, `foreach`, `repeat/until`, `goto` e `yield` não são construções
usáveis hoje. A existência de uma palavra-chave no lexer não implica que ela
tenha uma produção no parser.
