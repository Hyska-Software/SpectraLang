import sys

if hasattr(sys, "set_int_max_str_digits"):
    sys.set_int_max_str_digits(0)


def fatorial(n):
    """Empilha N..2 e desempilha multiplicando (LIFO: sai 2, 3, ..., N).

    Devolve (resultado, passos), onde cada passo e (fator, produto acumulado).
    """
    pilha = list(range(n, 1, -1))  # topo da pilha = 2 | base da pilha = N
    resultado = 1
    passos = []
    while pilha:
        fator = pilha.pop()
        resultado *= fator
        passos.append((fator, resultado))
    return resultado, passos


def main(argv):
    mostrar_traco = "--traco" in argv
    argumentos = [a for a in argv if a != "--traco"]

    try:
        n = int(argumentos[0]) if argumentos else int(input("Digite N: "))
    except ValueError:
        print("Entrada invalida: digite um numero inteiro.")
        return

    if n < 0:
        print("Nao existe fatorial de numero negativo.")
        return

    resultado, passos = fatorial(n)
    print(f"{n}! = {resultado}")

    if mostrar_traco:
        if not passos:
            print("sem passos: caso base (pilha vazia) -> 1")
            return
        largura = len(str(len(passos)))
        anterior = 1
        for i, (fator, parcial) in enumerate(passos, 1):
            print(f"{i:>{largura}}. desempilha {fator} : {anterior} x {fator} = {parcial}")
            anterior = parcial


if __name__ == "__main__":
    main(sys.argv[1:])
