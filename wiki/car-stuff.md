# Car Stuff

cost = $37000
apr = 5%
sales_tax = 4%
years = 6

months = years * 12

monthly = apr / 12
principal = cost * (1 + sales_tax)

payment = (monthly * principal) / (1 - (1 + monthly)^(-1 * months))

payment => $619.7178

## Current Costs

mileage = 27 miles / gallon
commute = 88 miles / day
gas_cost = $4.09 / gallon

cost = commute / mileage * gas_cost

cost * 5 days * 52 / 12 => $288.8247
