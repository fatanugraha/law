import requests

username = '1606862753'
password = '2719922682'
client_id = 'ebxtIDMpeJ7KKlG7rVmOHQB1wExyVdHT'
client_secret = 'QJG8gBzm2IM2kcC5MmrLZknsBtPhkd6o'


URL = "http://oauth.infralabs.cs.ui.ac.id/oauth/token"
payload = {
    'username': username,
    'password': password,
    'grant_type': 'password',
    'client_id': client_id,
    'client_secret': client_secret,
}

r = requests.post(url=URL, data=payload)
print(r.json()['access_token'])
