#!/usr/bin/env python3
import sys
import urllib.parse
import urllib.request
import json
from http.server import HTTPServer, BaseHTTPRequestHandler
import re

authorization_code = None

class OAuthCallbackHandler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def do_GET(self):
        global authorization_code
        parsed_url = urllib.parse.urlparse(self.path)
        query = parsed_url.query
        params = urllib.parse.parse_qs(query)
        
        if parsed_url.path == "/favicon.ico":
            self.send_response(404)
            self.end_headers()
            return

        if 'code' in params:
            authorization_code = params['code'][0]
            self.send_response(200)
            self.send_header('Content-Type', 'text/html')
            self.end_headers()
            self.wfile.write(b"<h1>Authorization Successful!</h1><p>Captured Box refresh token. You can close this window.</p>")
        else:
            self.send_response(200)
            self.send_header('Content-Type', 'text/html')
            self.end_headers()
            self.wfile.write(b"<h1>OAuth Server Active</h1><p>Waiting for callback code...</p>")

def exchange_code_for_token(client_id, client_secret, redirect_uri, code):
    url = "https://api.box.com/oauth2/token"
    data = urllib.parse.urlencode({
        'grant_type': 'authorization_code',
        'code': code,
        'client_id': client_id,
        'client_secret': client_secret,
        'redirect_uri': redirect_uri
    }).encode('utf-8')
    
    req = urllib.request.Request(url, data=data, headers={'Content-Type': 'application/x-www-form-urlencoded'})
    try:
        with urllib.request.urlopen(req) as response:
            return json.loads(response.read().decode('utf-8'))
    except Exception as e:
        print(f"Error exchanging token: {e}")
        if hasattr(e, 'read'):
            print(f"Details: {e.read().decode('utf-8')}")
        return None

def update_config_files(refresh_token):
    for filename in ['.toml', 'private_config.toml']:
        try:
            with open(filename, 'r') as f:
                content = f.read()
            # Replace refresh_token specifically under [box_credentials]
            pattern = r'(\[box_credentials\][\s\S]*?refresh_token\s*=\s*").*?(")'
            new_content = re.sub(pattern, rf'\g<1>{refresh_token}\g<2>', content)
            with open(filename, 'w') as f:
                f.write(new_content)
            print(f"Updated token in {filename}")
        except Exception as e:
            print(f"Could not update {filename}: {e}")

def main():
    if len(sys.argv) < 3:
        print("Usage: auto_box.py <client_id> <client_secret>")
        sys.exit(1)
        
    client_id = sys.argv[1]
    client_secret = sys.argv[2]
    redirect_uri = "http://localhost:8080/oauth/callback"
    port = 8080

    params = urllib.parse.urlencode({
        'response_type': 'code',
        'client_id': client_id,
        'redirect_uri': redirect_uri
    })
    auth_url = f"https://account.box.com/api/oauth2/authorize?{params}"
    
    print(f"AUTHORIZATION_URL:{auth_url}")
    sys.stdout.flush()

    server = HTTPServer(('localhost', port), OAuthCallbackHandler)
    while authorization_code is None:
        server.handle_request()

    print(f"Captured Code: {authorization_code}")
    sys.stdout.flush()

    tokens = exchange_code_for_token(client_id, client_secret, redirect_uri, authorization_code)
    if tokens and 'refresh_token' in tokens:
        refresh_token = tokens['refresh_token']
        print(f"REFRESH_TOKEN_SUCCESS:{refresh_token}")
        sys.stdout.flush()
        update_config_files(refresh_token)
    else:
        print("Failed to obtain tokens.")
        sys.stdout.flush()

if __name__ == "__main__":
    main()
