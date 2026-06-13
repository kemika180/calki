# Car Comparison

commute = 88 miles
level2 = 6 kWh / hr
price = $0.20 / kWh
gas = $4.09 / gallon

subaru_eff = 274 miles / 74.7 kWh => 3.668miles/kWh
chevy_eff = 300 miles / 102 kWh => 2.9412miles/kWh

subaru_power = commute / subaru_eff => 23.9912kWh
chevy_power = commute / chevy_eff => 29.92kWh

subaru_power / level2 => 3.9985hr
chevy_power / level2 => 4.9867hr

subaru_power * price => $4.7982
chevy_power * price => $5.984

## Current Car
mileage = 27 miles/gallon
gas / mileage * commute => $13.3304