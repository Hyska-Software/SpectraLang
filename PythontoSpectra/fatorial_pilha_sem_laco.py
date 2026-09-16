"""Fatorial com pilha explicita (LIFO), sem nenhum laco de repeticao (for/while).

Uso:
    python fatorial_pilha_sem_laco.py            # pergunta N
    python fatorial_pilha_sem_laco.py 50         # N = 50
    python fatorial_pilha_sem_laco.py 50 --traco # um passo por linha

Onde a versao anterior tinha laco, aqui ha recursao:
    while pilha:  ...   ->  desempilha(pilha, acumulado, passos)
    while passos: ...   ->  imprime_passos(passos, i)

Custo: a profundidade da recursao e O(N) (~N frames por fase), entao N acima de
~997 estoura o limite padrao do interpretador (RecursionError) -- limite que a
versao com laco nao tinha. sys.setrecursionlimit() aumenta, mas o ganho e
limitado pela C stack (risco de crash, nao de excecao).
"""

import sys

if hasattr(sys, "set_int_max_str_digits"):
    # CPython 3.11+ recusa converter int -> str acima de 4300 digitos.
    # Sem isto, N >= 1559 falha no print mesmo com o fatorial correto.
    sys.set_int_max_str_digits(0)


def empilha(n, pilha):
    """Empilha N, N-1, ..., 2. Uma chamada por elemento (sem laco)."""
    if n > 1:
        pilha.append(n)
        empilha(n - 1, pilha)


def desempilha(pilha, acumulado, passos):
    """Desempilha multiplicando. `passos` recebe (fator, produto) na ordem dos pop().

    Uma chamada por elemento desempilhado; o retorno da recursao e a "volta" da
    pilha (quem foi empilhado por ultimo sai primeiro).
    """
    if not pilha:
        return acumulado
    fator = pilha.pop()
    produto = acumulado * fator
    passos.append((fator, produto))
    return desempilha(pilha, produto, passos)


def fatorial(n):
    """Devolve (resultado, passos); cada passo e (fator desempilhado, produto acumulado)."""
    pilha = []
    empilha(n, pilha)
    passos = []
    return desempilha(pilha, 1, passos), passos


def imprime_passos(passos, i=0, anterior=1, largura=1):
    """Imprime um passo por linha: uma chamada recursiva por linha."""
    if i < len(passos):
        fator, produto = passos[i]
        print(f"{i + 1:>{largura}}. desempilha {fator} : {anterior} x {fator} = {produto}")
        imprime_passos(passos, i + 1, produto, largura)


def main(argv):
    mostrar_traco = "--traco" in argv
    argumentos = list(filter(lambda a: a != "--traco", argv))

    try:
        n = int(argumentos[0]) if argumentos else int(input("Digite N: "))
    except ValueError:
        print("Entrada invalida: digite um numero inteiro.")
        return

    if n < 0:
        print("Nao existe fatorial de numero negativo.")
        return

    try:
        resultado, passos = fatorial(n)
    except RecursionError:
        print(f"RecursionError: sem laco, a recursao desce ~{n} frames (limite: {sys.getrecursionlimit()}).")
        print(f"Para este N use a versao com laco: python fatorial_pilha.py {n}")
        return

    print(f"{n}! = {resultado}")

    if mostrar_traco:
        if not passos:
            print("sem passos: caso base (pilha vazia) -> 1")
            return
        imprime_passos(passos, largura=len(str(len(passos))))


if __name__ == "__main__":
    main(sys.argv[1:])
