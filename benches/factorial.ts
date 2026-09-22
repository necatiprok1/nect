function factorial(n: number): number {
    let result: number = 1;
    for (let i: number = 1; i <= n; i++) {
        result = result * i;
    }
    return result;
}

const n: number = 100;
const result: number = factorial(n);
console.log("factorial(" + n + ") = " + result);