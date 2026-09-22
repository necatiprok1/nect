// Numerical integration: Riemann sum for the integral of x^2 from 0 to 1
function integrate(n: number): number {
    let sum: number = 0.0;
    const h: number = 0.00001;
    for (let i: number = 0; i < n; i++) {
        const x: number = (i + 0.5) * h;
        sum = sum + x * x;
    }
    return sum;
}

const result: number = integrate(1000000);
console.log("numerical integration: " + result);