#!/usr/bin/env python3
import sys
import urllib.parse
import urllib.request
import json
import webbrowser
from http.server import HTTPServer, BaseHTTPRequestHandler

# Global variable to hold captured code
authorization_code = None

class OAuthCallbackHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        global authorization_code
        query = urllib.parse.urlparse(self.path).query
        params = urllib.parse.parse_qs(query)
        
        if 'code' in params:
            authorization_code = params['code'][0]
            self.send_response(200)
            self.send_header('Content-Type', 'text/html')
            self.end_headers()
            self.wfile.write(b"<h1>Authorization Successful!</h1><p>You can close this tab and return to the terminal.</p>")
        else:
            self.send_response(400)
            self.send_header('Content-Type', 'text/html')
            self.end_headers()
            self.wfile.write(b"<h1>Authorization Failed</h1><p>No authorization code found in the callback.</p>")

def run_local_server(port):
    server = HTTPServer(('localhost', port), OAuthCallbackHandler)
    print(f"Temporary server listening on http://localhost:{port}...")
    server.handle_request() # Wait for exactly one callback request

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
            res_data = json.loads(response.read().decode('utf-8'))
            return res_data
    except Exception as e:
        print(f"\nError exchanging code for token: {e}")
        if hasattr(e, 'read'):
            print(f"Details: {e.read().decode('utf-8')}")
        return None

def main():
    print("==================================================")
    print("      Box OAuth 2.0 Token Exchange Helper")
    print("==================================================")
    
    client_id = input("Enter your Box Client ID: ").strip()
    client_secret = input("Enter your Box Client Secret: ").strip()
    redirect_uri = input("Enter Redirect URI [http://localhost:8080/oauth/callback]: ").strip()
    if not redirect_uri:
        redirect_uri = "http://localhost:8080/oauth/callback"
        
    parsed_url = urllib.parse.urlparse(redirect_uri)
    port = parsed_url.port if parsed_url.port else 8080
    
    # 1. Construct Auth URL
    params = urllib.parse.urlencode({
        'response_type': 'code',
        'client_id': client_id,
        'redirect_uri': redirect_uri
    })
    auth_url = f"https://account.box.com/api/oauth2/authorize?{params}"
    
    print("\nOpening browser to authorize app...")
    webbrowser.open(auth_url)
    
    # 2. Wait for callback
    run_local_server(port)
    
    if not authorization_code:
        print("\nFailed to capture authorization code.")
        sys.exit(1)
        
    print(f"\nCaptured Authorization Code: {authorization_code}")
    print("Exchanging authorization code for token...")
    
    # 3. Exchange code for token
    token_response = exchange_code_for_token(client_id, client_secret, redirect_uri, authorization_code)
    
    if token_response:
        print("\nSuccess! Token details retrieved:")
        print("--------------------------------------------------")
        print(f"Access Token:  {token_response.get('access_token')}")
        print(f"Refresh Token: {token_response.get('refresh_token')}")
        print("--------------------------------------------------")
        print("\nCopy the Refresh Token and paste it into your configuration file.")
    else:
        print("\nFailed to retrieve tokens.")

if __name__ == "__main__":
    main()
