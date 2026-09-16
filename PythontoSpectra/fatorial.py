"""Fatorial por recursao, pilha (LIFO) e fila (FIFO).

As tres versoes multiplicam os mesmos fatores 2..N; a diferenca e a ORDEM em que
os fatores sao consumidos. Como a multiplicacao e comutativa, o resultado final
e o mesmo -- a estrutura de dados serve para deixar a ordem explicita.
"""

import sys
from collections import deque

if hasattr(sys, "set_int_max_str_digits"):
    # CPython 3.11+ recusa converter int -> str acima de 4300 digitos.
    # Sem isto, N >= 1559 falha no print mesmo com o fatorial correto.
    sys.set_int_max_str_digits(0)


def fatorial_recursivo(n):
    """A pilha aqui e implicita: a call stack do Python guarda N, N-1, ... ate o caso base."""
    if n > 1:
        return n * fatorial_recursivo(n - 1)
    return 1


def fatorial_pilha(n):
    """Empilha N..2 e desempilha multiplicando: fatores saem na ordem 2, 3, ..., N.

    E a volta da recursao (quem retorna primeiro e a chamada mais profunda),
    mas sem gastar a call stack do interpretador.
    """
    pilha = list(range(n, 1, -1))  # topo da pilha = 2
    parciais = [1]
    while pilha:
        parciais.append(parciais[-1] * pilha.pop())  # LIFO (pop)
    return parciais[-1], parciais


def fatorial_fila(n):
    """Enfileira N..2 e desenfileira multiplicando: fatores saem na ordem N, N-1, ..., 2.

    E a descida da recursao: o primeiro da fila e o N, o ultimo e o 2.
    """
    fila = deque(range(n, 1, -1))  # frente da fila = N
    parciais = [1]
    while fila:
        parciais.append(parciais[-1] * fila.popleft())  # FIFO (popleft)
    return parciais[-1], parciais


def main():
    try:
        n = int(input("Digite N: "))
    except ValueError:
        print("Entrada invalida: digite um numero inteiro.")
        return

    if n < 0:
        print("Nao existe fatorial de numero negativo.")
        return

    resultado_pilha, passos_pilha = fatorial_pilha(n)
    resultado_fila, passos_fila = fatorial_fila(n)

    print(f"recursivo : {n}! = {fatorial_recursivo(n)}")
    print(f"pilha     : {n}! = {resultado_pilha}  parciais: {' -> '.join(map(str, passos_pilha))}")
    print(f"fila      : {n}! = {resultado_fila}  parciais: {' -> '.join(map(str, passos_fila))}")


if __name__ == "__main__":
    main()
