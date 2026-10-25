import {render,screen} from "@testing-library/react";
import {MemoryRouter} from "react-router-dom";
import {NextAction} from "./NextAction";
it("shows a retry time without asking for manual recovery",()=>{render(<MemoryRouter><NextAction action={{kind:"retry",message:"Waiting",retry_at:Math.floor(Date.now()/1000)+240,href:"/settings/models",button:"Check models"}}/></MemoryRouter>);expect(screen.getByText(/Retrying in 4 minutes/)).toBeInTheDocument();expect(screen.getByRole("link",{name:"Check models"})).toHaveAttribute("href","/settings/models");});
