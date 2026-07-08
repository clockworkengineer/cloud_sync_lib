#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
get_dropbox_token.py

Automates retrieving a Dropbox OAuth2 refresh token.
"""

import http.server
import socketserver
import webbrowser
import urllib.parse
import urllib.request
import json
import re
import sys
import base64
from pathlib import Path

PORT = 8080
REDIRECT_URI = f"http://localhost:{PORT}"

class OAuthHandler(http.server.SimpleHTTPRequestHandler):
    """
    HTTP Request Handler to receive the OAuth2 authorization code callback.
    """
    auth_code = None

    def do_GET(self):
        """
        Handles the HTTP GET request containing the OAuth2 callback query params.
        """
        query = urllib.parse.urlparse(self.path).query
        params = urllib.parse.parse_qs(query)
        
        # Ignore favicon probes
        if urllib.parse.urlparse(self.path).path == "/favicon.ico":
            self.send_response(404)
            self.end_headers()
            return

        if "code" in params:
            OAuthHandler.auth_code = params["code"][0]
            self.send_response(200)
            self.send_header("Content-type", "text/html")
            self.end_headers()
            self.wfile.write(b"<html><body><h1>Authorization Successful!</h1><p>You can close this tab and return to the terminal.</p></body></html>")
        else:
            self.send_response(400)
            self.send_header("Content-type", "text/html")
            self.end_headers()
            self.wfile.write(b"<html><body><h1>Failed to authorize</h1></body></html>")

    def log_message(self, format, *args):
        """
        Suppresses log messages for local redirect requests.
        """
        pass # Suppress server logs

def run_server():
    """
    Runs a temporary local socket server to wait for the OAuth2 redirect callback.

    Returns:
        str: The retrieved authorization code.
    """
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("", PORT), OAuthHandler) as httpd:
        print(f"[*] Temporary server listening on {REDIRECT_URI}...")
        while OAuthHandler.auth_code is None:
            httpd.handle_request()
    return OAuthHandler.auth_code

def read_config():
    # Check private_config.toml first, then config.toml
    for filename in ["private_config.toml", "config.toml"]:
        path = Path(filename)
        if path.exists():
            content = path.read_text()
            sections = re.split(r'\n\s*\[([^\]]+)\]', content)
            for i in range(1, len(sections), 2):
                sec_name = sections[i].strip()
                sec_content = sections[i+1]
                if sec_name == "dropbox_credentials":
                    client_id_match = re.search(r'client_id\s*=\s*"([^"]+)"', sec_content)
                    client_secret_match = re.search(r'client_secret\s*=\s*"([^"]+)"', sec_content)
                    if client_id_match and client_secret_match:
                        cid = client_id_match.group(1)
                        csec = client_secret_match.group(1)
                        if "PLACEHOLDER" not in cid and "YOUR_APP_KEY" not in cid:
                            return cid, csec, filename
    return None, None, None

def update_config_files(refresh_token):
    for filename in ["config.toml", "private_config.toml"]:
        path = Path(filename)
        if path.exists():
            try:
                content = path.read_text()
                # Replace refresh_token specifically under [dropbox_credentials]
                pattern = r'(\[dropbox_credentials\][\s\S]*?refresh_token\s*=\s*").*?(")'
                new_content = re.sub(pattern, rf'\g<1>{refresh_token}\g<2>', content)
                path.write_text(new_content)
                print(f"[+] Successfully updated {filename} with the new refresh_token!")
            except Exception as e:
                print(f"[-] Could not update {filename}: {e}")

def exchange_code_for_token(code, client_id, client_secret):
    url = "https://api.dropboxapi.com/oauth2/token"
    
    data = urllib.parse.urlencode({
        "code": code,
        "grant_type": "authorization_code",
        "redirect_uri": REDIRECT_URI,
    }).encode("utf-8")
    
    # Prepare HTTP Basic Auth header for client_id:client_secret
    auth_str = f"{client_id}:{client_secret}"
    b64_auth_str = base64.b64encode(auth_str.encode("utf-8")).decode("utf-8")
    
    req = urllib.request.Request(url, data=data)
    req.add_header("Authorization", f"Basic {b64_auth_str}")
    req.add_header("Content-Type", "application/x-www-form-urlencoded")
    
    try:
        with urllib.request.urlopen(req) as response:
            res_data = json.loads(response.read().decode("utf-8"))
            return res_data.get("refresh_token")
    except Exception as e:
        print(f"Error exchanging code for token: {e}")
        if hasattr(e, 'read'):
            print(e.read().decode("utf-8"))
        sys.exit(1)

def main():
    print("==================================================")
    print("      Dropbox OAuth 2.0 Token Exchange Helper")
    print("==================================================")
    
    client_id, client_secret, config_file = read_config()
    
    if client_id and client_secret:
        print(f"[+] Auto-detected credentials in {config_file}")
    else:
        client_id = input("Enter your Dropbox App Key (Client ID): ").strip()
        client_secret = input("Enter your Dropbox App Secret (Client Secret): ").strip()
        
    # Build OAuth URL
    auth_url = (
        "https://www.dropbox.com/oauth2/authorize?"
        "response_type=code&"
        "token_access_type=offline&"
        f"redirect_uri={urllib.parse.quote(REDIRECT_URI)}&"
        f"client_id={client_id}"
    )
    
    print("\nOpening browser to authorize app...")
    try:
        webbrowser.open(auth_url)
    except Exception:
        pass
    print(f"If the browser did not open automatically, visit this URL:\n{auth_url}\n")
    
    # 2. Wait for callback
    code = run_server()
    
    if not code:
        print("[-] Error: Failed to capture authorization code.")
        sys.exit(1)
        
    print(f"\n[+] Captured Authorization Code: {code}")
    print("[*] Exchanging code for refresh token...")
    
    refresh_token = exchange_code_for_token(code, client_id, client_secret)
    
    if refresh_token:
        print(f"[+] Refresh Token: {refresh_token}")
        update_config_files(refresh_token)
    else:
        print("[-] Error: Did not receive a refresh token. Check your credentials and try again.")

if __name__ == "__main__":
    main()
