#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
get_box_token.py

Automates retrieving a Box OAuth2 refresh token by spinning up
a temporary local HTTP redirect server and opening the authorization page.
"""

import sys
import urllib.parse
import urllib.request
import json
import webbrowser
import re
from pathlib import Path
from http.server import HTTPServer, BaseHTTPRequestHandler

# Global variables and constants
authorization_code = None
PORT = 8080
REDIRECT_URI = f"http://localhost:{PORT}/oauth/callback"

class OAuthCallbackHandler(BaseHTTPRequestHandler):
    """
    HTTP Request Handler to receive the OAuth2 authorization code callback from Box.
    """
    def log_message(self, format, *args):
        """
        Suppresses console logging of requests to keep the output clean.
        """
        pass

    def do_GET(self):
        """
        Handles the HTTP GET request from Box's redirect containing the authorization code.
        """
        global authorization_code
        parsed_url = urllib.parse.urlparse(self.path)
        query = parsed_url.query
        params = urllib.parse.parse_qs(query)
        
        # Ignore favicon requests
        if parsed_url.path == "/favicon.ico":
            self.send_response(404)
            self.end_headers()
            return

        if 'code' in params:
            authorization_code = params['code'][0]
            self.send_response(200)
            self.send_header('Content-Type', 'text/html')
            self.end_headers()
            self.wfile.write(b"<h1>Authorization Successful!</h1><p>You can close this tab and return to the terminal.</p>")
        else:
            self.send_response(200)
            self.send_header('Content-Type', 'text/html')
            self.end_headers()
            self.wfile.write(b"<h1>OAuth Server Active</h1><p>Waiting for callback code...</p>")

def run_local_server(port):
    """
    Runs a temporary local web server to capture the authorization code.

    Args:
        port (int): The port number to listen on.
    """
    server = HTTPServer(('localhost', port), OAuthCallbackHandler)
    print(f"[*] Temporary server listening on http://localhost:{port}...")
    while authorization_code is None:
        server.handle_request()

def exchange_code_for_token(client_id, client_secret, redirect_uri, code):
    """
    Exchanges the authorization code for an OAuth2 token payload.

    Args:
        client_id (str): Box application client ID.
        client_secret (str): Box application client secret.
        redirect_uri (str): Redirect URI configured in Box.
        code (str): Authorization code captured from callback.

    Returns:
        dict: Parsed JSON response containing tokens if successful, or None.
    """
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

def read_config():
    """
    Reads config files to auto-detect Box credentials.

    Returns:
        tuple: (client_id, client_secret, source_filename) or (None, None, None).
    """
    for filename in ["private_config.toml", "config.toml"]:
        path = Path(filename)
        if path.exists():
            content = path.read_text()
            sections = re.split(r'\n\s*\[([^\]]+)\]', content)
            for i in range(1, len(sections), 2):
                sec_name = sections[i].strip()
                sec_content = sections[i+1]
                if sec_name == "box_credentials":
                    client_id_match = re.search(r'client_id\s*=\s*"([^"]+)"', sec_content)
                    client_secret_match = re.search(r'client_secret\s*=\s*"([^"]+)"', sec_content)
                    if client_id_match and client_secret_match:
                        cid = client_id_match.group(1)
                        csec = client_secret_match.group(1)
                        if "PLACEHOLDER" not in cid and "your_box" not in cid:
                            return cid, csec, filename
    return None, None, None

def update_config_files(refresh_token):
    """
    Updates the config files with the new refresh token.

    Args:
        refresh_token (str): The rotated refresh token to save.
    """
    for filename in ["config.toml", "private_config.toml"]:
        path = Path(filename)
        if path.exists():
            try:
                content = path.read_text()
                pattern = r'(\[box_credentials\][\s\S]*?refresh_token\s*=\s*").*?(")'
                new_content = re.sub(pattern, rf'\g<1>{refresh_token}\g<2>', content)
                path.write_text(new_content)
                print(f"[+] Successfully updated {filename} with the new refresh_token!")
            except Exception as e:
                print(f"[-] Could not update {filename}: {e}")

def main():
    """
    Main execution entry point.
    """
    print("==================================================")
    print("      Box OAuth 2.0 Token Exchange Helper")
    print("==================================================")
    
    client_id, client_secret, config_file = read_config()
    
    if client_id and client_secret:
        print(f"[+] Auto-detected credentials in {config_file}")
    else:
        client_id = input("Enter your Box Client ID: ").strip()
        client_secret = input("Enter your Box Client Secret: ").strip()
        
    redirect_uri = REDIRECT_URI
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
    try:
        webbrowser.open(auth_url)
    except Exception:
        pass
    print(f"If the browser did not open automatically, visit this URL:\n{auth_url}\n")
    
    # 2. Wait for callback
    run_local_server(port)
    
    if not authorization_code:
        print("\nFailed to capture authorization code.")
        sys.exit(1)
        
    print(f"\nCaptured Authorization Code: {authorization_code}")
    print("Exchanging authorization code for token...")
    
    # 3. Exchange code for token
    token_response = exchange_code_for_token(client_id, client_secret, redirect_uri, authorization_code)
    
    if token_response and 'refresh_token' in token_response:
        refresh_token = token_response.get('refresh_token')
        print("\nSuccess! Token details retrieved:")
        print("--------------------------------------------------")
        print(f"Access Token:  {token_response.get('access_token')}")
        print(f"Refresh Token: {refresh_token}")
        print("--------------------------------------------------")
        update_config_files(refresh_token)
    else:
        print("\nFailed to retrieve tokens.")

if __name__ == "__main__":
    main()
