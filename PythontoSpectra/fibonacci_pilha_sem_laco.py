"""Fibonacci com pilha explicita (LIFO), sem nenhum laco de repeticao (for/while).

Uso:
    python fibonacci_pilha_sem_laco.py            # pergunta N
    python fibonacci_pilha_sem_laco.py 10         # N = 10
    python fibonacci_pilha_sem_laco.py 10 --traco # um passo por linha

Diferenca em relacao ao fatorial: aqui o elemento desempilhado NAO e o operando,
e sim o indice do passo -- f(k) = f(k-1) + f(k-2). Os dois valores vivos da
recorrencia ficam em dois registradores (a, b), nao na pilha. O topo comeca em 2
e sobe ate N, entao o k desempilhado e exatamente o indice que esta sendo
formado; empilhar N..2 e desempilhar em ordem crescente e o que garante isso.

Custo: a profundidade da recursao e O(N) (~N frames por fase), entao N acima de
~997 estoura o limite padrao do interpretador (RecursionError).
"""

import sys

if hasattr(sys, "set_int_max_str_digits"):
    # CPython 3.11+ recusa converter int -> str acima de 4300 digitos
    # (fib(20575) ja passa disso). Sem isto, o print falha.
    sys.set_int_max_str_digits(0)


def empilha(n, pilha):
    """Empilha N, N-1, ..., 2: um elemento por passo da recorrencia (sem laco)."""
    if n > 1:
        pilha.append(n)
        empilha(n - 1, pilha)


def desempilha(pilha, a, b, passos):
    """Um passo por chamada: (a, b) = (b, a + b).

    Antes do passo: a = f(k-2), b = f(k-1). Depois: a = f(k-1), b = f(k).
    `passos` recebe (k, f(k-2), f(k-1), f(k)) na ordem dos pop().
    """
    if not pilha:
        return a, b
    k = pilha.pop()
    novo = a + b
    passos.append((k, a, b, novo))
    return desempilha(pilha, b, novo, passos)


def fibonacci(n):
    """Devolve (resultado, passos); f(0) = 0 e f(1) = 1 sao os casos base."""
    if n == 0:
        return 0, []
    pilha = []
    empilha(n, pilha)
    passos = []
    _, resultado = desempilha(pilha, 0, 1, passos)
    return resultado, passos


def imprime_passos(passos, i=0, largura=1):
    """Imprime um passo por linha: uma chamada recursiva por linha."""
    if i < len(passos):
        k, a, b, novo = passos[i]
        print(f"{i + 1:>{largura}}. desempilha {k} : {a} + {b} = {novo}")
        imprime_passos(passos, i + 1, largura)


def main(argv):
    mostrar_traco = "--traco" in argv
    argumentos = list(filter(lambda a: a != "--traco", argv))

    try:
        n = int(argumentos[0]) if argumentos else int(input("Digite N: "))
    except ValueError:
        print("Entrada invalida: digite um numero inteiro.")
        return

    if n < 0:
        print("Nao existe Fibonacci de numero negativo.")
        return

    try:
        resultado, passos = fibonacci(n)
    except RecursionError:
        print(f"RecursionError: sem laco, a recursao desce ~{n} frames (limite: {sys.getrecursionlimit()}).")
        print("Para este N use a versao iterativa (for) ou sys.setrecursionlimit().")
        return

    print(f"fib({n}) = {resultado}")

    if mostrar_traco:
        if not passos:
            print(f"sem passos: caso base fib({n}) = {resultado}")
            return
        imprime_passos(passos, largura=len(str(len(passos))))


if __name__ == "__main__":
    main(sys.argv[1:])
