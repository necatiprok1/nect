// Numerical integration: Riemann sum for the integral of x^2 from 0 to 1
function integrate(n) {
    let sum = 0.0;
    const h = 0.00001;
    for (let i = 0; i < n; i++) {
        const x = (i + 0.5) * h;
        sum = sum + x * x;
    }
    return sum;
}

const result = integrate(1000000);
console.log("numerical integration: " + result);