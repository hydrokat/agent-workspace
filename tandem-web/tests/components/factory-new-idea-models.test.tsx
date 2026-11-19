import { afterEach, describe, expect, it } from "bun:test";
import React from "react";
import { act, cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { useDom } from "../helpers/dom";
import { NewIdeaDialog } from "../../src/components/factory/new-idea-dialog";
import type { FactoryCli, Idea, ModelOption } from "../../src/domain/models/factory";

useDom();

afterEach(() => {
  cleanup();
});

const IDEA = { id: "i-abcd1234" } as unknown as Idea;

function setValue(el: HTMLInputElement | HTMLSelectElement, value: string) {
  fireEvent.focus(el);
  fireEvent.change(el, { target: { value } });
  if (el instanceof window.HTMLInputElement) {
    const nativeSetter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value"
    )?.set;
    nativeSetter?.call(el, value);
    fireEvent.keyUp(el);
  }
}

describe("NewIdeaDialog orchestrator selection", () => {
  it("offers claude, agy, and opencode with no hardcoded model default", () => {
    const { getByLabelText } = render(
      <NewIdeaDialog open={true} onOpenChange={() => {}} onStartIdea={async () => IDEA} />
    );
    const select = getByLabelText("CLI") as HTMLSelectElement;
    expect(Array.from(select.options).map((o) => o.value)).toEqual(["claude", "agy", "opencode"]);
    expect((getByLabelText(/Model/) as HTMLInputElement).value).toBe("");
  });

  it("loads models when the CLI changes and fills the model from the picker", async () => {
    const requested: FactoryCli[] = [];
    let submitted: { cli: string; model: string } | null = null;
    const loadModels = async (cli: FactoryCli): Promise<ModelOption[]> => {
      requested.push(cli);
      return cli === "opencode"
        ? [
            { id: "opencode/big-pickle", label: "Big Pickle" },
            { id: "openai/gpt-5.4#high", label: "GPT 5.4 high" },
          ]
        : [{ id: "opus", label: "Opus" }];
    };
    const { getByLabelText, getByRole, findByText } = render(
      <NewIdeaDialog
        open={true}
        onOpenChange={() => {}}
        loadModels={loadModels}
        onStartIdea={async (input) => {
          submitted = input;
          return IDEA;
        }}
      />
    );
    await waitFor(() => expect(requested).toEqual(["claude"]));

    await act(async () => {
      setValue(getByLabelText("CLI") as HTMLSelectElement, "opencode");
    });
    await waitFor(() => expect(requested).toEqual(["claude", "opencode"]));
    await findByText("opencode/big-pickle");

    const options = within(getByRole("listbox")).getAllByRole("option");
    expect(options.length).toBe(2);
    await act(async () => {
      fireEvent.click(options[1].querySelector("button")!);
    });
    expect((getByLabelText(/Model/) as HTMLInputElement).value).toBe("openai/gpt-5.4#high");

    await act(async () => {
      fireEvent.submit((getByLabelText(/Model/) as HTMLInputElement).closest("form")!);
    });
    await waitFor(() => expect(submitted).toEqual({ cli: "opencode", model: "openai/gpt-5.4#high", title: undefined } as never));
    expect(getByRole("dialog")).toBeTruthy();
  });

  it("filters the model list by the typed text", async () => {
    const loadModels = async (): Promise<ModelOption[]> => [
      { id: "opencode/big-pickle", label: "Big Pickle" },
      { id: "openai/gpt-5.4", label: "GPT" },
    ];
    const { getByLabelText, findByText, getByRole } = render(
      <NewIdeaDialog open={true} onOpenChange={() => {}} loadModels={loadModels} onStartIdea={async () => IDEA} />
    );
    await findByText("opencode/big-pickle");
    await act(async () => {
      setValue(getByLabelText(/Model/) as HTMLInputElement, "gpt");
    });
    expect(within(getByRole("listbox")).getAllByRole("option").length).toBe(1);
  });

  it("falls back to manual entry with a warning when listing fails", async () => {
    const loadModels = async (): Promise<ModelOption[]> => {
      throw new Error("boom");
    };
    const { findByRole, getByLabelText } = render(
      <NewIdeaDialog open={true} onOpenChange={() => {}} loadModels={loadModels} onStartIdea={async () => IDEA} />
    );
    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("Could not list models");
    expect((getByLabelText(/Model/) as HTMLInputElement).disabled).toBe(false);
  });

  it("rejects non provider/model ids for opencode", async () => {
    let called = false;
    const { getByLabelText, findByRole } = render(
      <NewIdeaDialog
        open={true}
        onOpenChange={() => {}}
        onStartIdea={async () => {
          called = true;
          return IDEA;
        }}
      />
    );
    await act(async () => {
      setValue(getByLabelText("CLI") as HTMLSelectElement, "opencode");
      setValue(getByLabelText(/Model/) as HTMLInputElement, "claude-sonnet-4-6");
    });
    await act(async () => {
      fireEvent.submit((getByLabelText(/Model/) as HTMLInputElement).closest("form")!);
    });
    expect((await findByRole("alert")).textContent).toContain("provider/model");
    expect(called).toBe(false);
  });
});
